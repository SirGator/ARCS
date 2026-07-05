/**
 * @file interpretation_service.cpp
 * @brief Implements InterpretationService, the pipeline stage responsible for
 *        turning free-text input into a structured interpretation proposal by
 *        calling out to an external interpretation worker API (when configured),
 *        and for wrapping the resulting payload into an interpretation_proposal
 *        artifact linked back to the originating ingress event.
 */

#include "core/services/interpretation_service.hpp"

#include <chrono>
#include <iomanip>
#include <sstream>

#include "artifact/factory.hpp"
#include "interpretation/worker_adapter.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 string (YYYY-MM-DDTHH:MM:SSZ).
 * @return The formatted current UTC timestamp.
 */
std::string utc_now()
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

} // namespace

InterpretationService::InterpretationService()
    : InterpretationService([](const arcs::interpretation::InterpretationApiConfig& config) {
        return std::make_unique<arcs::adapters::interpretation::WorkerInterpretationAdapter>(config);
    })
{
}

InterpretationService::InterpretationService(AdapterFactory adapter_factory)
    : adapter_factory_(std::move(adapter_factory))
{
}

/**
 * @brief Sends free-text input to the configured external interpretation worker
 *        API and returns its structured response, logging and returning a
 *        failure outcome if the config is missing, the call fails, or the
 *        response payload is not a JSON object.
 * @param input The raw free-text input to interpret.
 * @param interpretation_config Pointer to the interpretation API configuration;
 *        if null or missing an interpret_api_url, interpretation is skipped and
 *        a default (unsuccessful) outcome is returned.
 * @param logger The system logger used to record failures.
 * @return An InterpretationOutcome with ok=true and the response payload on
 *         success, or ok=false with diagnostic log_output on failure.
 */
InterpretationOutcome InterpretationService::interpret(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config,
    const arcs::schema::SchemaRegistry& schema_registry,
    arcs::core::SystemLogger& logger) const
{
    if (interpretation_config == nullptr || !interpretation_config->interpret_api_url.has_value()) {
        return {};
    }

    const auto* schema_entry = schema_registry.find_schema("arcs.interpretation_proposal.v1");
    if (schema_entry == nullptr) {
        logger.fail("interpret", "schema arcs.interpretation_proposal.v1 not found");
        return {.ok = false, .payload = std::nullopt, .log_output = "step: interpret -> FAIL | schema missing\n"};
    }

    auto adapter = adapter_factory_(*interpretation_config);
    const arcs::interpretation::InterpretationRequest input_request{
        .request_id = "req_free_text",
        .raw_input = input,
        .schema_id = "arcs.interpretation_proposal.v1",
        .schema = schema_entry->document,
        .context = nlohmann::json{{"timezone", "Europe/Berlin"}, {"language", "de"}, {"current_time", utc_now()}},
        .prompt_config = nlohmann::json{{"mode", "strict_json"}, {"temperature", 0.0}},
    };

    auto input_response = adapter->interpret(input_request);
    std::ostringstream output;
    if (!input_response.ok) {
        const auto error = input_response.error.value_or("unknown error");
        logger.fail("interpret", error);
        output << "step: interpret -> FAIL | " << error << '\n';
        return {.ok = false, .payload = std::nullopt, .log_output = output.str()};
    }

    output << "step: interpret -> OK\n";
    if (!input_response.request_id.empty()) {
        output << "interpretation request_id: " << input_response.request_id << '\n';
    }
    if (!input_response.schema_id.empty()) {
        output << "interpretation schema_id: " << input_response.schema_id << '\n';
    }
    if (!input_response.payload.is_object()) {
        logger.fail("interpretation artifact", "payload is not an object");
        output << "step: interpretation artifact -> FAIL | payload is not an object\n";
        return {.ok = false, .payload = std::nullopt, .log_output = output.str()};
    }

    output << "step: interpretation artifact -> OK\n";
    return {.ok = true, .payload = input_response.payload, .log_output = output.str()};
}

/**
 * @brief Wraps an interpretation payload into an "interpretation_proposal"
 *        artifact, linking it back to the originating ingress event.
 * @param ingress_event The ingress artifact that triggered the interpretation.
 * @param interpretation_payload The structured interpretation payload to attach.
 * @return The constructed interpretation_proposal artifact version, with
 *         provenance referencing the ingress event.
 */
arcs::artifact::ArtifactVersion InterpretationService::make_proposal_artifact(
    const arcs::artifact::ArtifactVersion& ingress_event,
    const nlohmann::json& interpretation_payload) const
{
    auto artifact = arcs::artifact::factory::make_base_artifact(
        "interpretation_proposal",
        "arcs.interpretation_proposal.v1",
        ingress_event.stream_key,
        "system",
        "interpretation_worker",
        "api",
        "interpret",
        "low",
        "external",
        utc_now());

    artifact.payload = interpretation_payload;
    artifact.provenance.parents = {ingress_event.artifact_id};
    artifact.provenance.rules_applied = {"external_interpretation"};
    artifact.provenance.transform = "interpret_free_text";
    return artifact;
}

} // namespace arcs::core::services

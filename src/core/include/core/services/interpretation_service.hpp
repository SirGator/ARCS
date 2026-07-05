/**
 * @file interpretation_service.hpp
 * @brief Interprets ingested input (optionally via an external
 *        interpretation API) into a structured payload and wraps it as a
 *        proposal artifact.
 */

#pragma once

#include <functional>
#include <memory>
#include <optional>
#include <string>

#include "adapters/interpretation/interpretation_adapter.hpp"
#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"
#include "schema/schema_registry.hpp"
#include "core/system_logger.hpp"
#include "interpretation/config.hpp"

namespace arcs::core::services {

/**
 * @brief Result of interpreting input: whether interpretation succeeded,
 *        the resulting JSON payload if so, and any log output produced.
 */
struct InterpretationOutcome {
    bool ok{false};
    std::optional<nlohmann::json> payload;
    std::string log_output;
};

/**
 * @brief Interprets raw input into a structured JSON payload and packages
 *        the result as a proposal artifact.
 */
class InterpretationService {
public:
    using AdapterFactory = std::function<std::unique_ptr<arcs::adapters::interpretation::IInterpretationAdapter>(
        const arcs::interpretation::InterpretationApiConfig& config)>;

    InterpretationService();
    explicit InterpretationService(AdapterFactory adapter_factory);

    /**
     * @brief Interprets the given input, using the optional interpretation
     *        API configuration.
     * @param input Raw input text to interpret.
     * @param interpretation_config Optional interpretation API
     *        configuration; pass nullptr to use the default.
     * @param logger Logger used to record interpretation steps.
     * @return The interpretation outcome.
     */
    InterpretationOutcome interpret(
        const std::string& input,
        const arcs::interpretation::InterpretationApiConfig* interpretation_config,
        const arcs::schema::SchemaRegistry& schema_registry,
        arcs::core::SystemLogger& logger) const;

    /**
     * @brief Wraps an interpretation payload into a proposal artifact linked
     *        to its originating ingress event.
     * @param ingress_event Ingress event the interpretation was derived from.
     * @param interpretation_payload Structured interpretation result.
     * @return The constructed proposal artifact.
     */
    arcs::artifact::ArtifactVersion make_proposal_artifact(
        const arcs::artifact::ArtifactVersion& ingress_event,
        const nlohmann::json& interpretation_payload) const;

private:
    AdapterFactory adapter_factory_;
};

} // namespace arcs::core::services

/**
 * @file planning_service.cpp
 * @brief Implements PlanningService, the pipeline stage that turns a task artifact and
 *        the current policy into a concrete, executable option (plan) artifact.
 *
 * Currently produces a single "generate report" option describing the steps, required
 * permissions/scopes, and policy reference needed to execute it.
 */
#include "core/services/planning_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include "artifact/factory.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Formats the current UTC time as an ISO-8601 timestamp string ("%Y-%m-%dT%H:%M:%SZ").
 * @return The current UTC timestamp string.
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

/**
 * @brief Builds a base artifact version populated with the given identity, provenance,
 *        and trust metadata, timestamped at the current UTC time.
 * @param type Artifact type name.
 * @param schema_id Schema identifier the artifact conforms to.
 * @param stream_key Stream key the artifact belongs to.
 * @param actor_type Type of the actor producing the artifact.
 * @param actor_id Identifier of the actor producing the artifact.
 * @param source_kind Kind of source that originated the artifact.
 * @param source_ref Reference to the originating source.
 * @param trust_level Trust level assigned to the artifact.
 * @param trust_source_class Trust source classification.
 * @return The newly constructed base ArtifactVersion.
 */
arcs::artifact::ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class)
{
    return arcs::artifact::factory::make_base_artifact(
        type,
        schema_id,
        stream_key,
        actor_type,
        actor_id,
        source_kind,
        source_ref,
        trust_level,
        trust_source_class,
        utc_now());
}

} // namespace

/**
 * @brief Creates the "generate report" option artifact derived from a task, describing
 *        a single emit_report step, its required permissions/scopes, and linking back
 *        to the task and policy artifacts it was derived from.
 * @param task_artifact The task artifact the option is being planned for.
 * @param policy_ref The policy artifact the option must comply with.
 * @param input The original request text to embed in the option payload.
 * @return The derived "option" artifact version.
 */
arcs::artifact::ArtifactVersion PlanningService::create_report_option(
    const arcs::artifact::ArtifactVersion& task_artifact,
    const arcs::artifact::ArtifactVersion& policy_ref,
    const std::string& input) const
{
    auto option = make_artifact(
        "option",
        "arcs.option.v1",
        task_artifact.stream_key,
        "system",
        "kernel",
        "internal",
        "task_to_option",
        "high",
        "system");
    option.payload = nlohmann::json{
        {"title", "Generate report"},
        {"human_summary", "Emit a JSON report summarizing the interpreted input."},
        {"safety_level", "low"},
        {"request", input},
        {"policy_ref", {
            {"artifact_id", policy_ref.artifact_id},
            {"version_id", policy_ref.version_id},
        }},
        {"requires_permissions", {"exec:report_emit"}},
        {"required_scopes", {task_artifact.stream_key}},
        {"steps", nlohmann::json::array({nlohmann::json{
            {"kind", "emit_report"},
            {"params", {
                {"format", "json"},
                {"sections", {"summary", "risks"}},
            }},
        }})},
    };
    option.provenance.parents = {task_artifact.artifact_id, policy_ref.artifact_id};
    option.provenance.rules_applied = {"materialize_option"};
    option.provenance.transform = "derive_option";
    return option;
}

} // namespace arcs::core::services

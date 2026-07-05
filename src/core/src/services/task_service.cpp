/**
 * @file task_service.cpp
 * @brief Implements TaskService, the pipeline stage that derives a "task" artifact from
 *        an ingress event, the raw input text, and the parsed input flags (approval,
 *        permission, policy drift).
 */
#include "core/services/task_service.hpp"

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
 * @brief Derives a "task" artifact from an ingress event and the parsed user input,
 *        carrying forward the approval/permission/policy-drift flags into its payload
 *        and linking it back to the originating ingress event.
 * @param ingress_event The ingress event artifact that triggered this task.
 * @param input Raw input text to store as the task description.
 * @param parsed_input Parsed flags (approval, permission, policy drift) extracted from input.
 * @return The derived "task" artifact version.
 */
arcs::artifact::ArtifactVersion TaskService::create_task(
    const arcs::artifact::ArtifactVersion& ingress_event,
    const std::string& input,
    const ParsedInput& parsed_input) const
{
    auto task_artifact = make_artifact(
        "task",
        "arcs.task.v1",
        ingress_event.stream_key,
        "system",
        "kernel",
        "internal",
        "input",
        "high",
        "system");
    task_artifact.payload = nlohmann::json{
        {"title", "Input task"},
        {"description", input},
        {"approval", parsed_input.approval_yes},
        {"permission", parsed_input.permission_yes},
        {"policy_drift", parsed_input.policy_drift},
    };
    task_artifact.provenance.parents = {ingress_event.artifact_id};
    task_artifact.provenance.rules_applied = {"task_from_input"};
    task_artifact.provenance.transform = "derive_task_from_input";
    return task_artifact;
}

} // namespace arcs::core::services

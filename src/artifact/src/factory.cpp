/**
 * @file factory.cpp
 * @brief Implements make_base_artifact(), which assembles a new
 *        ArtifactVersion populated with generated ids and metadata.
 */
#include "artifact/factory.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include "artifact/ids.hpp"

namespace arcs::artifact::factory {

namespace {

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 timestamp
 *        (e.g. "2026-07-04T12:00:00Z").
 * @return The formatted UTC timestamp string.
 */
std::string utc_now()
{
    const auto now = std::chrono::system_clock::now();
    const std::time_t time = std::chrono::system_clock::to_time_t(now);
    const std::tm tm = *std::gmtime(&time);

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

} // namespace

/**
 * @brief Builds a new ArtifactVersion with freshly generated artifact and
 *        version identifiers, version set to 1, and the given metadata
 *        filled in.
 * @param type Artifact type name (e.g. "ingress_event").
 * @param schema_id Identifier of the schema the payload should conform to.
 * @param stream_key Logical stream/session the artifact belongs to.
 * @param created_by_actor_type Type of the actor creating the artifact.
 * @param created_by_id Identifier of the actor creating the artifact.
 * @param source_kind Kind of source the data originated from.
 * @param source_ref Concrete reference to the source.
 * @param trust_level Trust level to assign.
 * @param trust_source_class Trust source classification.
 * @param created_at ISO-8601 UTC timestamp to use; if empty, the current
 *        UTC time is used instead.
 * @return A newly constructed ArtifactVersion with the given metadata.
 */
ArtifactVersion make_base_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& created_by_actor_type,
    const std::string& created_by_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class,
    const std::string& created_at)
{
    ArtifactVersion artifact{};
    artifact.artifact_id = ids::new_artifact_id();
    artifact.version_id = ids::new_version_id();
    artifact.version = 1;
    artifact.type = type;
    artifact.schema_id = schema_id;
    artifact.schema_version = 1;
    artifact.created_at = created_at.empty() ? utc_now() : created_at;
    artifact.created_by = {created_by_actor_type, created_by_id};
    artifact.source = {source_kind, source_ref};
    artifact.trust = {trust_level, trust_source_class};
    artifact.stream_key = stream_key;
    return artifact;
}

} // namespace arcs::artifact::factory

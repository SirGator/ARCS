/**
 * @file semantic_event_factory.cpp
 * @brief Implements SemanticEventFactory, which builds semantic Event
 * records describing changes to artifacts.
 */
#include "core/commit/semantic_event_factory.hpp"

#include "artifact/ids.hpp"

namespace arcs::core::commit {

/**
 * @brief Constructs a semantic event referencing the given artifact,
 * stamping a freshly generated event id, the given type and timestamp, the
 * artifact's creator as actor, and the artifact's stream key.
 * @param event_type Type of the semantic event (e.g. "created", "head_advanced").
 * @param artifact Artifact the event refers to.
 * @param timestamp Timestamp to stamp on the event.
 * @return The constructed Event.
 */
arcs::event::Event SemanticEventFactory::make_event(
    const std::string& event_type,
    const arcs::artifact::ArtifactVersion& artifact,
    const std::string& timestamp,
    const CommitContext& commit_context) const
{
    arcs::event::Event event{};
    event.event_id = arcs::artifact::ids::new_event_id();
    event.event_type = event_type;
    event.ts = timestamp;
    event.actor = artifact.created_by;
    event.refs.push_back(arcs::event::EventRef{
        .artifact_id = artifact.artifact_id,
        .version_id = artifact.version_id,
        .role = "target",
    });
    event.stream_key = artifact.stream_key;
    event.payload = {
        {"artifact_type", artifact.type},
        {"schema_id", artifact.schema_id},
        {"commit_id", commit_context.commit_id},
        {"stage", commit_context.stage},
        {"correlation_id", commit_context.correlation_id},
        {"commit_actor", commit_context.actor},
        {"reason", commit_context.reason},
        {"cause_refs", nlohmann::json::array()},
    };
    for (const auto& cause_ref : commit_context.cause_refs) {
        event.payload["cause_refs"].push_back({
            {"artifact_id", cause_ref.artifact_id},
            {"version_id", cause_ref.version_id},
            {"role", cause_ref.role},
        });
    }
    return event;
}

} // namespace arcs::core::commit

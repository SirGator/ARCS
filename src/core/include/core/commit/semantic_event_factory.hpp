/**
 * @file semantic_event_factory.hpp
 * @brief Factory for constructing semantic events that describe an artifact
 *        version change for the event stream.
 */

#pragma once

#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "event/event.hpp"

namespace arcs::core::commit {

struct CommitContext {
    std::string commit_id;
    std::string stage;
    std::string correlation_id;
    std::string actor;
    std::string reason;
    std::vector<arcs::event::EventRef> cause_refs;
};

/**
 * @brief Creates semantic `Event` records that describe artifact version
 *        changes, for inclusion in commit bundles.
 */
class SemanticEventFactory {
public:
    /**
     * @brief Builds a semantic event describing a change to an artifact.
     * @param event_type Type identifier for the event.
     * @param artifact Artifact version the event refers to.
     * @param timestamp Timestamp to record on the event.
     * @param commit_context Shared commit metadata to embed in the event payload.
     * @return The constructed event.
     */
    arcs::event::Event make_event(
        const std::string& event_type,
        const arcs::artifact::ArtifactVersion& artifact,
        const std::string& timestamp,
        const CommitContext& commit_context) const;
};

} // namespace arcs::core::commit

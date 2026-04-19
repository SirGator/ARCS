#pragma once

#include <string>
#include <unordered_map>
#include <vector>

#include "artifact/artifact.hpp"
#include "event/event.hpp"
#include "store/store.hpp"

namespace arcs::store::head_tracker {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;
using arcs::event::EventRef;
using arcs::store::CommitRejectedError;

// Applies exactly one `head_advanced` event.
// Preconditions:
// - `event.event_type == "head_advanced"`
// - the target reference exists
// - `target.version_id` exists in `versions_by_version_id`
// - `target.artifact_id` matches the referenced version
void apply_head_advanced_event(
    const Event& event,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

// Applies all head-relevant events in order.
// Non-`head_advanced` events are ignored.
void apply_events(
    const std::vector<Event>& events,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

} // namespace arcs::store::head_tracker

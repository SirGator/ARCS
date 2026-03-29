#pragma once

#include "artifact/artifact.hpp"
#include "event/event.hpp"
#include "store/store.hpp"

#include <string>
#include <unordered_map>
#include <vector>

namespace arcs::store::head_tracker {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;
using arcs::event::EventRef;
using arcs::store::CommitRejectedError;

// Wendet genau ein head_advanced-Event an.
// Voraussetzungen:
// - event.event_type == "head_advanced"
// - target-Ref existiert
// - target.version_id existiert in versions_by_version_id
// - target.artifact_id passt zur referenzierten Version
void apply_head_advanced_event(
    const Event& event,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

// Wendet alle head-relevanten Events in Reihenfolge an.
// Nicht-head_advanced-Events werden ignoriert.
void apply_events(
    const std::vector<Event>& events,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

} // namespace arcs::store::head_tracker

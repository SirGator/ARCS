/**
 * @file head_tracker.hpp
 * @brief Reducer logic that derives artifact "head" pointers from the event log.
 *
 * A store's notion of the "current head" of an artifact is not the latest
 * appended version; it is whatever the most recent `head_advanced` event
 * says it is. This module applies those events to a
 * `artifact_id -> head version_id` map, shared by both StoreMemory and
 * StoreSqlite so the semantics stay identical across backends.
 */

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

/**
 * @brief Applies exactly one `head_advanced` event to the head map.
 *
 * Preconditions:
 * - `event.event_type == "head_advanced"`
 * - the target reference exists
 * - `target.version_id` exists in `versions_by_version_id`
 * - `target.artifact_id` matches the referenced version
 *
 * @param event The event to apply; must be of type `head_advanced`.
 * @param versions_by_version_id Lookup of known versions, used to validate the target.
 * @param head_by_artifact_id Head map to update in place with the new head.
 */
void apply_head_advanced_event(
    const Event& event,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

/**
 * @brief Applies all head-relevant events in order.
 *
 * Non-`head_advanced` events are ignored.
 *
 * @param events Ordered list of events to fold over.
 * @param versions_by_version_id Lookup of known versions, used to validate targets.
 * @param head_by_artifact_id Head map to update in place as events are applied.
 */
void apply_events(
    const std::vector<Event>& events,
    const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
    std::unordered_map<std::string, std::string>& head_by_artifact_id);

} // namespace arcs::store::head_tracker

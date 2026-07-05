/**
 * @file commit.hpp
 * @brief Data types describing an atomic write request against the store.
 *
 * Defines `PendingVersion` (a new artifact version plus optional optimistic
 * lock context) and `CommitBundle` (the all-or-nothing collection of
 * versions and events that make up a single commit).
 */

#pragma once

#include <optional>
#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "event/event.hpp"

namespace arcs::store::commit {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;

/**
 * @brief A planned new version plus optional optimistic-lock context.
 *
 * `expected_head_version_id` means:
 * "I may only commit this new version if the current head
 *  of this artifact is exactly this version."
 */
struct PendingVersion {
    ArtifactVersion version;
    std::optional<std::string> expected_head_version_id;
};

/**
 * @brief The store's atomic write bundle.
 *
 * Either everything is committed, or nothing is.
 */
struct CommitBundle {
    std::vector<PendingVersion> versions;
    std::vector<Event> events;
};

} // namespace arcs::store::commit

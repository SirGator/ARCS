#pragma once

#include "store/commit.hpp"
#include "store/store.hpp"

namespace arcs::store::optimistic_lock {

using arcs::store::CommitBundle;
using arcs::store::CommitRejectedError;
using arcs::store::IStore;
using arcs::store::commit::PendingVersion;

// Validates a single `PendingVersion` against the current head in the store.
//
// Rules:
// - `expected_head_version_id` not set:
//     -> no lock check
// - `expected_head_version_id` set:
//     -> a current head must exist
//     -> the current head must exactly match `expected_head_version_id`
//
// On violation, throws `CommitRejectedError`.
void validate_pending_version(
    const PendingVersion& pending,
    const IStore& store);

// Validates all `PendingVersion` entries in a `CommitBundle`.
//
// Throws `CommitRejectedError` on the first violation.
void validate_bundle(
    const CommitBundle& bundle,
    const IStore& store);

} // namespace arcs::store::optimistic_lock

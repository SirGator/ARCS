/**
 * @file optimistic_lock.hpp
 * @brief Optimistic-concurrency-control checks for commit bundles.
 *
 * Validates that a `PendingVersion`'s expected head (if any) matches the
 * store's actual current head before allowing a commit to proceed, so that
 * concurrent writers racing on the same artifact are detected rather than
 * silently overwriting each other.
 */

#pragma once

#include "store/commit.hpp"
#include "store/store.hpp"

namespace arcs::store::optimistic_lock {

using arcs::store::CommitBundle;
using arcs::store::CommitRejectedError;
using arcs::store::IStore;
using arcs::store::commit::PendingVersion;

/**
 * @brief Validates a single `PendingVersion` against the current head in the store.
 *
 * Rules:
 * - `expected_head_version_id` not set:
 *     -> no lock check
 * - `expected_head_version_id` set:
 *     -> a current head must exist
 *     -> the current head must exactly match `expected_head_version_id`
 *
 * @param pending The pending version whose lock expectation is checked.
 * @param store The store to read the current head from.
 * @throws CommitRejectedError if the lock expectation is violated.
 */
void validate_pending_version(
    const PendingVersion& pending,
    const IStore& store);

/**
 * @brief Validates all `PendingVersion` entries in a `CommitBundle`.
 *
 * @param bundle The commit bundle whose versions are checked.
 * @param store The store to read the current head from.
 * @throws CommitRejectedError on the first violation.
 */
void validate_bundle(
    const CommitBundle& bundle,
    const IStore& store);

} // namespace arcs::store::optimistic_lock

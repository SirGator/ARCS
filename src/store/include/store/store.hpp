/**
 * @file store.hpp
 * @brief Core storage abstraction for artifacts and events (`IStore`).
 *
 * Declares the append-only, event-sourced storage interface implemented by
 * both `StoreMemory` and `StoreSqlite`, plus the store-level error
 * hierarchy and the query filter type used by `list()`.
 */

#pragma once

#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "event/event.hpp"
#include "store/commit.hpp"

namespace arcs::store {

using arcs::artifact::ArtifactVersion;
using arcs::event::Event;
using commit::CommitBundle;
using commit::PendingVersion;

// ---------------------------------
// Error Types
// ---------------------------------

/** @brief Base class for all errors raised by the store implementations. */
class StoreError : public std::runtime_error {
public:
    explicit StoreError(const std::string& message)
        : std::runtime_error(message) {}
};

/** @brief Raised when a lookup (artifact, version, head) finds nothing. */
class NotFoundError : public StoreError {
public:
    explicit NotFoundError(const std::string& message)
        : StoreError(message) {}
};

/** @brief Raised when a write is rejected by validation or optimistic-lock checks. */
class CommitRejectedError : public StoreError {
public:
    explicit CommitRejectedError(const std::string& message)
        : StoreError(message) {}
};

// ---------------------------------
// Filter for list()
// ---------------------------------

/** @brief Optional filter criteria applied by `IStore::list()`. */
struct ListQuery {
    std::optional<std::string> type;
    std::optional<std::string> stream_key;
};

// ---------------------------------
// IStore
// ---------------------------------

/**
 * @brief Abstract, append-only store for artifact versions and events.
 *
 * Implementations (`StoreMemory`, `StoreSqlite`) provide the same
 * semantics: writes either go through the atomic `commit()` boundary, or
 * (for debug-only use) via the single-item `append_*` methods that bypass
 * commit-time validation and optimistic locking. Reads expose both the
 * derived "current head" of an artifact and the raw version/event log.
 */
class IStore {
public:
    virtual ~IStore() = default;

    // Append-only single operations.
    // WARNING: bypasses the commit boundary; debug only.

    /**
     * @brief Appends a single artifact version directly, bypassing the commit boundary.
     * @param version The artifact version to append.
     */
    virtual void append_artifact(const ArtifactVersion& version) = 0;

    /**
     * @brief Appends a single event directly, bypassing the commit boundary.
     * @param event The event to append.
     */
    virtual void append_event(const Event& event) = 0;

    /**
     * @brief Central atomic operation: either all versions and events are
     * committed, or nothing is.
     * @param bundle The versions and events to commit together.
     */
    virtual void commit(const store::CommitBundle& bundle) = 0;

    /**
     * @brief Returns the current head version of an artifact.
     *
     * Important: the head is not automatically the "latest version";
     * it is derived from head semantics and `head_advanced` events.
     *
     * @param artifact_id The artifact whose head to fetch.
     * @return The artifact version currently designated as head.
     */
    virtual ArtifactVersion get(const std::string& artifact_id) const = 0;

    /**
     * @brief Fetches an exact version by `version_id`.
     * @param version_id The version identifier to look up.
     * @return The matching artifact version.
     */
    virtual ArtifactVersion get_version(const std::string& version_id) const = 0;

    /**
     * @brief Lists all versions, optionally filtered.
     *
     * Result order is deterministic for debug/test output, not log order.
     *
     * @param query Optional filter on type and/or stream_key.
     * @return The matching artifact versions.
     */
    virtual std::vector<ArtifactVersion> list(const ListQuery& query = {}) const = 0;

    /**
     * @brief Lists all events in stable log order, optionally filtered by stream.
     * @param stream_key If set, restrict the result to this stream.
     * @return The matching events in log order.
     */
    virtual std::vector<Event> list_events(
        const std::optional<std::string>& stream_key = std::nullopt
    ) const = 0;

    // Helper methods for existence checks.

    /**
     * @brief Checks whether any version exists for the given artifact.
     * @param artifact_id The artifact identifier to check.
     * @return True if at least one version exists.
     */
    virtual bool has_artifact(const std::string& artifact_id) const = 0;

    /**
     * @brief Checks whether a version with the given id exists.
     * @param version_id The version identifier to check.
     * @return True if the version exists.
     */
    virtual bool has_version(const std::string& version_id) const = 0;

    /**
     * @brief Returns the current head version id of an artifact, if any.
     *
     * Optional, but useful for tests, locking, and debugging.
     *
     * @param artifact_id The artifact whose head version id to fetch.
     * @return The head version id, or std::nullopt if the artifact has no head.
     */
    virtual std::optional<std::string> current_head_version_id(
        const std::string& artifact_id
    ) const = 0;
};

} // namespace arcs::store

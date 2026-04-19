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

class StoreError : public std::runtime_error {
public:
    explicit StoreError(const std::string& message)
        : std::runtime_error(message) {}
};

class NotFoundError : public StoreError {
public:
    explicit NotFoundError(const std::string& message)
        : StoreError(message) {}
};

class CommitRejectedError : public StoreError {
public:
    explicit CommitRejectedError(const std::string& message)
        : StoreError(message) {}
};

// ---------------------------------
// Filter for list()
// ---------------------------------

struct ListQuery {
    std::optional<std::string> type;
    std::optional<std::string> stream_key;
};

// ---------------------------------
// IStore
// ---------------------------------

class IStore {
public:
    virtual ~IStore() = default;

    // Append-only single operations.
    // WARNING: bypasses the commit boundary; debug only.
    virtual void append_artifact(const ArtifactVersion& version) = 0;
    virtual void append_event(const Event& event) = 0;

    // Central atomic operation:
    // either all versions and events are committed, or nothing is.
    virtual void commit(const store::CommitBundle& bundle) = 0;

    // Current head of an artifact.
    // Important: the head is not automatically the "latest version";
    // it is derived from head semantics and `head_advanced` events.
    virtual ArtifactVersion get(const std::string& artifact_id) const = 0;

    // Exact version by `version_id`.
    virtual ArtifactVersion get_version(const std::string& version_id) const = 0;

    // All versions, optionally filtered.
    // Deterministic debug/test output, not log order.
    virtual std::vector<ArtifactVersion> list(const ListQuery& query = {}) const = 0;

    // All events in stable log order.
    virtual std::vector<Event> list_events(
        const std::optional<std::string>& stream_key = std::nullopt
    ) const = 0;

    // Helper methods for existence checks.
    virtual bool has_artifact(const std::string& artifact_id) const = 0;
    virtual bool has_version(const std::string& version_id) const = 0;

    // Optional, but useful for tests, locking, and debugging.
    virtual std::optional<std::string> current_head_version_id(
        const std::string& artifact_id
    ) const = 0;
};

} // namespace arcs::store

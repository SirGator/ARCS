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
// Fehlerarten
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
// Filter für list()
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

    // Append-only Einzeloperationen.
    // WARNING: bypasses commit boundary - debug only.
    virtual void append_artifact(const ArtifactVersion& version) = 0;
    virtual void append_event(const Event& event) = 0;

    // Zentrale atomare Operation:
    // entweder alle Versionen + Events werden übernommen, oder nichts.
    virtual void commit(const store::CommitBundle& bundle) = 0;

    // Aktueller Head eines Artefakts.
    // Wichtig: Head ist nicht automatisch "neueste Version",
    // sondern ergibt sich aus der Head-Semantik / head_advanced-Events.
    virtual ArtifactVersion get(const std::string& artifact_id) const = 0;

    // Exakte Version über version_id.
    virtual ArtifactVersion get_version(const std::string& version_id) const = 0;

    // Alle Versionen, optional gefiltert.
    // Deterministische Debug-/Test-Ausgabe, nicht Log-Reihenfolge.
    virtual std::vector<ArtifactVersion> list(const ListQuery& query = {}) const = 0;

    // Alle Events in stabiler Log-Reihenfolge.
    virtual std::vector<Event> list_events(
        const std::optional<std::string>& stream_key = std::nullopt
    ) const = 0;

    // Hilfsmethoden für Existenzprüfungen.
    virtual bool has_artifact(const std::string& artifact_id) const = 0;
    virtual bool has_version(const std::string& version_id) const = 0;

    // Optional, aber praktisch für Tests / Locking / Debugging.
    virtual std::optional<std::string> current_head_version_id(
        const std::string& artifact_id
    ) const = 0;
};

} // namespace arcs::store

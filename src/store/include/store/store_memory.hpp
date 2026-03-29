#pragma once

#include "store/store.hpp"

#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <optional>
#include <string>

namespace arcs::store {

class StoreMemory final : public IStore {
public:
    StoreMemory();
    ~StoreMemory() override;

    // ---------------------------------
    // Write Operations
    // WARNING: bypasses commit boundary - debug only.
    // ---------------------------------

    void append_artifact(const ArtifactVersion& version) override;
    void append_event(const Event& event) override;

    void commit(const CommitBundle& bundle) override;

    // ---------------------------------
    // Read Operations
    // list() is deterministic for debugging/tests, not commit/log order.
    // ---------------------------------

    ArtifactVersion get(const std::string& artifact_id) const override;
    ArtifactVersion get_version(const std::string& version_id) const override;

    std::vector<ArtifactVersion> list(const ListQuery& query = {}) const override;

    std::vector<Event> list_events(
        const std::optional<std::string>& stream_key = std::nullopt
    ) const override;

    // ---------------------------------
    // Existence Checks
    // ---------------------------------

    bool has_artifact(const std::string& artifact_id) const override;
    bool has_version(const std::string& version_id) const override;

    std::optional<std::string> current_head_version_id(
        const std::string& artifact_id
    ) const override;

private:
    // ---------------------------------
    // Validation Helpers
    // ---------------------------------

    static void ensure_version_insertable(
        const ArtifactVersion& version,
        const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id
    );

    void ensure_version_insertable(const ArtifactVersion& version) const;

    static void ensure_event_insertable(
        const Event& event,
        const std::unordered_set<std::string>& known_event_ids
    );

    void ensure_bundle_locally_consistent(const CommitBundle& bundle) const;

    // ---------------------------------
    // State Mutation Helpers
    // ---------------------------------

    static void append_artifact_to_state(
        const ArtifactVersion& version,
        std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
        std::unordered_map<std::string, std::vector<std::string>>& version_ids_by_artifact_id
    );

private:
    // ---------------------------------
    // Internal State (append-only)
    // ---------------------------------

    // version_id -> ArtifactVersion
    std::unordered_map<std::string, ArtifactVersion> versions_by_version_id_;

    // artifact_id -> [version_id...]
    std::unordered_map<std::string, std::vector<std::string>> version_ids_by_artifact_id_;

    // Event Log (append-only, Reihenfolge wichtig!)
    std::vector<Event> event_log_;

    // schnelle Duplicate-Prüfung für Events
    std::unordered_set<std::string> event_ids_;

    // artifact_id -> current head version_id
    std::unordered_map<std::string, std::string> head_by_artifact_id_;
};

} // namespace arcs::store

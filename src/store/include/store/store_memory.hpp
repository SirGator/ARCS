/**
 * @file store_memory.hpp
 * @brief In-memory `IStore` implementation used for tests and debugging.
 *
 * Keeps all artifact versions, events, and derived heads in unordered maps
 * with no persistence. Enforces the same schema-validation, uniqueness,
 * and optimistic-lock rules as `StoreSqlite`, making it a fast drop-in
 * substitute for the persistent backend.
 */

#pragma once

#include <optional>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "store/store.hpp"

namespace arcs::store {

/**
 * @brief Non-persistent `IStore` backed by in-process hash maps.
 *
 * Commits are applied by mutating copies of the internal state and only
 * swapping them in if every version/event in the bundle passes validation,
 * giving all-or-nothing commit semantics without a real transaction log.
 */
class StoreMemory final : public IStore {
public:
    StoreMemory();
    ~StoreMemory() override;

    // ---------------------------------
    // Write Operations
    // WARNING: bypasses the commit boundary; debug only.
    // ---------------------------------

    void append_artifact(const ArtifactVersion& version) override;
    void append_event(const Event& event) override;

    void commit(const CommitBundle& bundle) override;

    // ---------------------------------
    // Read Operations
    // `list()` is deterministic for debugging/tests, not commit/log order.
    // ---------------------------------

    ArtifactVersion get(const std::string& artifact_id) const override;
    ArtifactVersion get_version(const std::string& version_id) const override;

    std::vector<ArtifactVersion> list(const ListQuery& query = {}) const override;

    std::vector<Event> list_events(
        const std::optional<std::string>& stream_key = std::nullopt
    ) const override;

    // ---------------------------------
    // Existence checks.
    // ---------------------------------

    bool has_artifact(const std::string& artifact_id) const override;
    bool has_version(const std::string& version_id) const override;

    std::optional<std::string> current_head_version_id(
        const std::string& artifact_id
    ) const override;

private:
    // ---------------------------------
    // Validation helpers.
    // ---------------------------------

    /**
     * @brief Validates that a version can be inserted into the given map (no
     * duplicate version_id, non-empty ids, schema-valid).
     * @param version The candidate version.
     * @param versions_by_version_id The map to check for duplicates against.
     * @throws CommitRejectedError if the version is invalid or a duplicate.
     */
    static void ensure_version_insertable(
        const ArtifactVersion& version,
        const std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id
    );

    /**
     * @brief Validates a version against this instance's current state.
     * @param version The candidate version.
     * @throws CommitRejectedError if the version is invalid or a duplicate.
     */
    void ensure_version_insertable(const ArtifactVersion& version) const;

    /**
     * @brief Validates that an event can be inserted (no duplicate event_id,
     * non-empty ids, schema-valid).
     * @param event The candidate event.
     * @param known_event_ids Set of already-known event ids to check against.
     * @throws CommitRejectedError if the event is invalid or a duplicate.
     */
    static void ensure_event_insertable(
        const Event& event,
        const std::unordered_set<std::string>& known_event_ids
    );

    /**
     * @brief Validates a whole commit bundle for internal consistency
     * (non-empty, no duplicate ids within the bundle itself).
     * @param bundle The bundle to validate.
     * @throws CommitRejectedError on the first violation found.
     */
    void ensure_bundle_locally_consistent(const CommitBundle& bundle) const;

    // ---------------------------------
    // State mutation helpers.
    // ---------------------------------

    /**
     * @brief Inserts a version into the given version maps.
     * @param version The version to insert.
     * @param versions_by_version_id Map to insert the version into, keyed by version_id.
     * @param version_ids_by_artifact_id Map to append the version_id into, keyed by artifact_id.
     */
    static void append_artifact_to_state(
        const ArtifactVersion& version,
        std::unordered_map<std::string, ArtifactVersion>& versions_by_version_id,
        std::unordered_map<std::string, std::vector<std::string>>& version_ids_by_artifact_id
    );

private:
    // ---------------------------------
    // Internal state (append-only).
    // ---------------------------------

    // `version_id` -> `ArtifactVersion`
    std::unordered_map<std::string, ArtifactVersion> versions_by_version_id_;

    // `artifact_id` -> `[version_id...]`
    std::unordered_map<std::string, std::vector<std::string>> version_ids_by_artifact_id_;

    // Event log (append-only, order matters!).
    std::vector<Event> event_log_;

    // Fast duplicate check for events.
    std::unordered_set<std::string> event_ids_;

    // `artifact_id` -> current head `version_id`
    std::unordered_map<std::string, std::string> head_by_artifact_id_;
};

} // namespace arcs::store

#pragma once

#include <memory>
#include <string>

#include "store/store.hpp"

struct sqlite3;

namespace arcs::store {

// Persistent implementation of IStore backed by a single SQLite file.
//
// Design goals (per Phase-0 plan in docs/phases/phase-0-foundation.md):
//   - Append-only artifact and event tables.
//   - One DB-level transaction per commit() (commit-boundary rule).
//   - Head table is derived from the `head_advanced` event log, not from
//     "latest version" (reducer-rule for head).
//   - Optimistic locking: same `expected_head_version_id` semantics as
//     StoreMemory (delegated to optimistic_lock::validate_bundle).
//   - Single-writer concurrency: the writer takes BEGIN IMMEDIATE.
//
// Fail-closed: any DB error throws StoreError. Caller decides retry policy.
class StoreSqlite final : public IStore {
public:
    // Opens (and migrates) a database at `path`. If `path` is ":memory:",
    // a private in-memory database is created. Throws StoreError on failure.
    explicit StoreSqlite(const std::string& path);
    ~StoreSqlite() override;

    StoreSqlite(const StoreSqlite&) = delete;
    StoreSqlite& operator=(const StoreSqlite&) = delete;

    // ---------------------------------
    // Write Operations
    // WARNING: bypasses the commit boundary; debug only.
    // ---------------------------------

    void append_artifact(const ArtifactVersion& version) override;
    void append_event(const Event& event) override;

    void commit(const CommitBundle& bundle) override;

    // ---------------------------------
    // Read Operations
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

    // Access for tests / observability.
    const std::string& path() const noexcept { return path_; }

private:
    void ensure_schema();

    // Throws StoreError with sqlite error code appended.
    [[noreturn]] void throw_sqlite_error(const char* context) const;

    // Returns next ordinal in the append log.
    std::int64_t next_ordinal() const;

    // Transaction helpers.
    void begin_immediate();
    void commit_transaction();
    void rollback_transaction();

    sqlite3* db_{nullptr};
    std::string path_;
};

} // namespace arcs::store

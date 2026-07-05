/**
 * @file store_sqlite.hpp
 * @brief Persistent `IStore` implementation backed by a single SQLite file.
 */

#pragma once

#include <memory>
#include <string>

#include "store/store.hpp"

struct sqlite3;

namespace arcs::store {

/**
 * @brief Persistent implementation of IStore backed by a single SQLite file.
 *
 * Design goals follow the repository's current ARCS Core specification and
 * append-only store rules.
 *   - Append-only artifact and event tables.
 *   - One DB-level transaction per commit() (commit-boundary rule).
 *   - Head table is derived from the `head_advanced` event log, not from
 *     "latest version" (reducer-rule for head).
 *   - Optimistic locking: same `expected_head_version_id` semantics as
 *     StoreMemory (delegated to optimistic_lock::validate_bundle).
 *   - Single-writer concurrency: the writer takes BEGIN IMMEDIATE.
 *
 * Fail-closed: any DB error throws StoreError. Caller decides retry policy.
 */
class StoreSqlite final : public IStore {
public:
    /**
     * @brief Opens (and migrates) a database at `path`.
     *
     * If `path` is ":memory:", a private in-memory database is created.
     *
     * @param path Filesystem path to the SQLite database, or ":memory:".
     * @throws StoreError on failure to open or migrate the database.
     */
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

    /** @brief Returns the filesystem path (or ":memory:") this store was opened with. */
    const std::string& path() const noexcept { return path_; }

private:
    /**
     * @brief Creates the schema (tables/indexes) if it does not already exist.
     * @throws StoreError on any DDL failure.
     */
    void ensure_schema();

    /**
     * @brief Throws a StoreError with the given context and the current sqlite error message appended.
     * @param context Short description of the operation that failed.
     */
    [[noreturn]] void throw_sqlite_error(const char* context) const;

    /**
     * @brief Returns the next ordinal in the global append log, incrementing the stored counter.
     * @return The freshly allocated ordinal value.
     */
    std::int64_t next_ordinal() const;

    // Transaction helpers.

    /** @brief Begins an immediate write transaction (`BEGIN IMMEDIATE`). */
    void begin_immediate();
    /** @brief Commits the current transaction. */
    void commit_transaction();
    /** @brief Rolls back the current transaction, swallowing any sqlite error. */
    void rollback_transaction();

    sqlite3* db_{nullptr};
    std::string path_;
};

} // namespace arcs::store

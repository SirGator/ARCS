#include "store/store_sqlite.hpp"

#include "artifact/json.hpp"
#include "event/json.hpp"
#include "schema/schema_loader.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validator.hpp"
#include "store/head_tracker.hpp"
#include "store/optimistic_lock.hpp"

#include <sqlite3.h>

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <unordered_map>
#include <unordered_set>

namespace arcs::store {

namespace {

const arcs::schema::SchemaRegistry& artifact_base_registry()
{
    static const auto registry = [] {
        arcs::schema::SchemaRegistry registry;
        const auto schema_path = std::filesystem::path(__FILE__).parent_path()
            .parent_path().parent_path().parent_path()
            / "schemas" / "v1" / "artifact_base.schema.json";

        const auto schema_entry = arcs::schema::SchemaLoader::load_from_file(schema_path);
        if (!schema_entry.has_value() || !registry.register_schema(*schema_entry)) {
            throw arcs::store::StoreError(
                "store schema gate misconfigured: artifact_base schema could not be loaded");
        }

        return registry;
    }();

    return registry;
}

void ensure_base_artifact_valid(const ArtifactVersion& version)
{
    const nlohmann::json artifact_json = version;
    const auto validation = arcs::schema::Validator::validate(
        artifact_json,
        "arcs.artifact_base.v1",
        artifact_base_registry());

    if (!validation.valid) {
        std::string message = "artifact version rejected: base schema validation failed";
        if (!validation.errors.empty()) {
            message += " at " + validation.errors.front().path + ": " + validation.errors.front().message;
        }
        throw CommitRejectedError(message);
    }
}

// RAII wrapper for sqlite3_stmt.
struct Stmt {
    sqlite3_stmt* stmt{nullptr};

    Stmt() = default;
    explicit Stmt(sqlite3_stmt* s) : stmt(s) {}
    ~Stmt() {
        if (stmt) {
            sqlite3_finalize(stmt);
        }
    }
    Stmt(const Stmt&) = delete;
    Stmt& operator=(const Stmt&) = delete;
};

// Prepares a statement and throws StoreError with sqlite context on failure.
Stmt prepare_stmt(sqlite3* db, const char* sql)
{
    sqlite3_stmt* p = nullptr;
    const int rc = sqlite3_prepare_v2(db, sql, -1, &p, nullptr);
    if (rc != SQLITE_OK) {
        std::string message = "prepare failed: ";
        message += sql;
        message += " | ";
        message += sqlite3_errmsg(db);
        if (p) {
            sqlite3_finalize(p);
        }
        throw StoreError(message);
    }
    return Stmt(p);
}

// Bind a string parameter. SQLITE_TRANSIENT tells sqlite to copy the value.
#ifndef SQLITE_TRANSIENT
#define SQLITE_TRANSIENT reinterpret_cast<sqlite3_destructor_type>(-1)
#endif

void bind_text(sqlite3_stmt* stmt, int index, const std::string& value)
{
    if (sqlite3_bind_text(stmt, index, value.c_str(),
                          static_cast<int>(value.size()),
                          SQLITE_TRANSIENT) != SQLITE_OK) {
        throw StoreError(std::string("bind_text failed: ") + sqlite3_errstr(sqlite3_errcode(sqlite3_db_handle(stmt))));
    }
}

void bind_int64(sqlite3_stmt* stmt, int index, std::int64_t value)
{
    if (sqlite3_bind_int64(stmt, index, value) != SQLITE_OK) {
        throw StoreError(std::string("bind_int64 failed: ") + sqlite3_errstr(sqlite3_errcode(sqlite3_db_handle(stmt))));
    }
}

std::string column_text(sqlite3_stmt* stmt, int index)
{
    const unsigned char* p = sqlite3_column_text(stmt, index);
    if (p == nullptr) {
        return {};
    }
    int len = sqlite3_column_bytes(stmt, index);
    return std::string(reinterpret_cast<const char*>(p), static_cast<std::size_t>(len));
}

void exec_or_throw(sqlite3* db, const char* sql)
{
    char* err = nullptr;
    if (sqlite3_exec(db, sql, nullptr, nullptr, &err) != SQLITE_OK) {
        std::string message = err ? err : "unknown sqlite error";
        sqlite3_free(err);
        throw StoreError(std::string("sqlite exec failed: ") + message);
    }
}

} // namespace

StoreSqlite::StoreSqlite(const std::string& path)
    : path_(path)
{
    if (sqlite3_open(path.c_str(), &db_) != SQLITE_OK) {
        std::string message = "sqlite3_open failed";
        if (db_) {
            message += std::string(": ") + sqlite3_errmsg(db_);
            sqlite3_close(db_);
            db_ = nullptr;
        }
        throw StoreError(message);
    }

    // Single-writer concurrency. Foreign keys off (we don't have relations).
    exec_or_throw(db_, "PRAGMA journal_mode=WAL;");
    exec_or_throw(db_, "PRAGMA synchronous=NORMAL;");
    exec_or_throw(db_, "PRAGMA foreign_keys=OFF;");

    ensure_schema();
}

StoreSqlite::~StoreSqlite()
{
    if (db_) {
        sqlite3_close(db_);
        db_ = nullptr;
    }
}

void StoreSqlite::throw_sqlite_error(const char* context) const
{
    throw StoreError(std::string(context) + ": " + sqlite3_errmsg(db_));
}

void StoreSqlite::ensure_schema()
{
    static const char* kCreateSchema =
        "CREATE TABLE IF NOT EXISTS schema_meta ("
        "  version INTEGER NOT NULL"
        ");"

        "CREATE TABLE IF NOT EXISTS artifact_versions ("
        "  version_id TEXT PRIMARY KEY,"
        "  artifact_id TEXT NOT NULL,"
        "  version INTEGER NOT NULL,"
        "  type TEXT NOT NULL,"
        "  schema_id TEXT NOT NULL,"
        "  schema_version INTEGER NOT NULL,"
        "  created_at TEXT NOT NULL,"
        "  created_by_json TEXT NOT NULL,"
        "  source_json TEXT NOT NULL,"
        "  trust_json TEXT NOT NULL,"
        "  stream_key TEXT NOT NULL,"
        "  tags_json TEXT NOT NULL,"
        "  payload_json TEXT NOT NULL,"
        "  provenance_json TEXT NOT NULL,"
        "  ordinal INTEGER NOT NULL,"
        "  UNIQUE(artifact_id, version)"
        ");"

        "CREATE INDEX IF NOT EXISTS idx_artifact_versions_by_artifact "
        "  ON artifact_versions(artifact_id, ordinal);"

        "CREATE TABLE IF NOT EXISTS events ("
        "  event_id TEXT PRIMARY KEY,"
        "  event_type TEXT NOT NULL,"
        "  ts TEXT NOT NULL,"
        "  actor_json TEXT NOT NULL,"
        "  refs_json TEXT NOT NULL,"
        "  stream_key TEXT NOT NULL,"
        "  payload_json TEXT NOT NULL,"
        "  prev_hash TEXT NOT NULL DEFAULT '',"
        "  ordinal INTEGER NOT NULL"
        ");"

        "CREATE INDEX IF NOT EXISTS idx_events_stream "
        "  ON events(stream_key, ordinal);"

        "CREATE TABLE IF NOT EXISTS artifact_heads ("
        "  artifact_id TEXT PRIMARY KEY,"
        "  current_head_version_id TEXT NOT NULL"
        ");"

        "CREATE TABLE IF NOT EXISTS ordinal_seq ("
        "  name TEXT PRIMARY KEY,"
        "  value INTEGER NOT NULL"
        ");"

        "INSERT OR IGNORE INTO schema_meta(version) VALUES (1);"
        "INSERT OR IGNORE INTO ordinal_seq(name, value) VALUES ('global', 0);";

    exec_or_throw(db_, kCreateSchema);
}

std::int64_t StoreSqlite::next_ordinal() const
{
    Stmt stmt = prepare_stmt(
        db_,
        "UPDATE ordinal_seq SET value = value + 1 "
        "WHERE name = 'global' "
        "RETURNING value;");

    if (sqlite3_step(stmt.stmt) != SQLITE_ROW) {
        throw_sqlite_error("step next_ordinal");
    }

    return sqlite3_column_int64(stmt.stmt, 0);
}

void StoreSqlite::begin_immediate()
{
    exec_or_throw(db_, "BEGIN IMMEDIATE;");
}

void StoreSqlite::commit_transaction()
{
    exec_or_throw(db_, "COMMIT;");
}

void StoreSqlite::rollback_transaction()
{
    char* err = nullptr;
    sqlite3_exec(db_, "ROLLBACK;", nullptr, nullptr, &err);
    if (err) {
        sqlite3_free(err);
    }
}

void StoreSqlite::append_artifact(const ArtifactVersion& version)
{
    if (version.artifact_id.empty()) {
        throw CommitRejectedError("artifact version rejected: artifact_id is empty");
    }
    if (version.version_id.empty()) {
        throw CommitRejectedError("artifact version rejected: version_id is empty");
    }

    begin_immediate();
    try {
        // Check duplicate.
        {
            Stmt check = prepare_stmt(
                db_,
                "SELECT 1 FROM artifact_versions WHERE version_id = ?1;");
            bind_text(check.stmt, 1, version.version_id);
            if (sqlite3_step(check.stmt) == SQLITE_ROW) {
                rollback_transaction();
                throw CommitRejectedError(
                    "artifact version rejected: duplicate version_id '" + version.version_id + "'");
            }
        }

        const std::int64_t ord = next_ordinal();

        nlohmann::json created_by_j = version.created_by;
        nlohmann::json source_j    = version.source;
        nlohmann::json trust_j     = version.trust;
        nlohmann::json tags_j      = version.tags;
        nlohmann::json provenance_j = version.provenance;
        nlohmann::json payload_j   = version.payload;

        Stmt insert = prepare_stmt(
            db_,
            "INSERT INTO artifact_versions("
            "  version_id, artifact_id, version, type, schema_id, schema_version,"
            "  created_at, created_by_json, source_json, trust_json, stream_key,"
            "  tags_json, payload_json, provenance_json, ordinal"
            ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15);");

        bind_text(insert.stmt, 1, version.version_id);
        bind_text(insert.stmt, 2, version.artifact_id);
        bind_int64(insert.stmt, 3, version.version);
        bind_text(insert.stmt, 4, version.type);
        bind_text(insert.stmt, 5, version.schema_id);
        bind_int64(insert.stmt, 6, version.schema_version);
        bind_text(insert.stmt, 7, version.created_at);
        bind_text(insert.stmt, 8, created_by_j.dump());
        bind_text(insert.stmt, 9, source_j.dump());
        bind_text(insert.stmt, 10, trust_j.dump());
        bind_text(insert.stmt, 11, version.stream_key);
        bind_text(insert.stmt, 12, tags_j.dump());
        bind_text(insert.stmt, 13, payload_j.dump());
        bind_text(insert.stmt, 14, provenance_j.dump());
        bind_int64(insert.stmt, 15, ord);

        if (sqlite3_step(insert.stmt) != SQLITE_DONE) {
            throw_sqlite_error("step insert version");
        }

        commit_transaction();
    } catch (...) {
        rollback_transaction();
        throw;
    }
}

void StoreSqlite::append_event(const Event& event)
{
    if (event.event_id.empty()) {
        throw CommitRejectedError("event rejected: event_id is empty");
    }
    if (event.event_type.empty()) {
        throw CommitRejectedError("event rejected: event_type is empty");
    }

    begin_immediate();
    try {
        {
            Stmt check = prepare_stmt(
                db_,
                "SELECT 1 FROM events WHERE event_id = ?1;");
            bind_text(check.stmt, 1, event.event_id);
            if (sqlite3_step(check.stmt) == SQLITE_ROW) {
                rollback_transaction();
                throw CommitRejectedError(
                    "event rejected: duplicate event_id '" + event.event_id + "'");
            }
        }

        const std::int64_t ord = next_ordinal();

        nlohmann::json actor_j = event.actor;
        nlohmann::json refs_j = event.refs;
        nlohmann::json payload_j = event.payload;

        Stmt insert = prepare_stmt(
            db_,
            "INSERT INTO events("
            "  event_id, event_type, ts, actor_json, refs_json,"
            "  stream_key, payload_json, prev_hash, ordinal"
            ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);");

        bind_text(insert.stmt, 1, event.event_id);
        bind_text(insert.stmt, 2, event.event_type);
        bind_text(insert.stmt, 3, event.ts);
        bind_text(insert.stmt, 4, actor_j.dump());
        bind_text(insert.stmt, 5, refs_j.dump());
        bind_text(insert.stmt, 6, event.stream_key);
        bind_text(insert.stmt, 7, payload_j.dump());
        bind_text(insert.stmt, 8, event.prev_hash);
        bind_int64(insert.stmt, 9, ord);

        if (sqlite3_step(insert.stmt) != SQLITE_DONE) {
            throw_sqlite_error("step insert event");
        }

        commit_transaction();
    } catch (...) {
        rollback_transaction();
        throw;
    }
}

namespace {

ArtifactVersion load_artifact_version(sqlite3_stmt* stmt)
{
    ArtifactVersion v;
    v.version_id        = column_text(stmt, 0);
    v.artifact_id       = column_text(stmt, 1);
    v.version           = static_cast<int>(sqlite3_column_int64(stmt, 2));
    v.type              = column_text(stmt, 3);
    v.schema_id         = column_text(stmt, 4);
    v.schema_version    = static_cast<int>(sqlite3_column_int64(stmt, 5));
    v.created_at        = column_text(stmt, 6);
    v.stream_key        = column_text(stmt, 7);

    auto created_by_j   = nlohmann::json::parse(column_text(stmt, 8));
    auto source_j       = nlohmann::json::parse(column_text(stmt, 9));
    auto trust_j        = nlohmann::json::parse(column_text(stmt, 10));
    auto tags_j         = nlohmann::json::parse(column_text(stmt, 11));
    auto payload_j      = nlohmann::json::parse(column_text(stmt, 12));
    auto provenance_j   = nlohmann::json::parse(column_text(stmt, 13));

    from_json(created_by_j, v.created_by);
    from_json(source_j, v.source);
    from_json(trust_j, v.trust);
    if (tags_j.is_array()) {
        v.tags = tags_j.get<std::vector<std::string>>();
    }
    v.payload = payload_j;
    from_json(provenance_j, v.provenance);

    return v;
}

Event load_event(sqlite3_stmt* stmt)
{
    Event e;
    e.event_id   = column_text(stmt, 0);
    e.event_type = column_text(stmt, 1);
    e.ts         = column_text(stmt, 2);
    e.stream_key = column_text(stmt, 3);
    e.prev_hash  = column_text(stmt, 4);

    auto actor_j  = nlohmann::json::parse(column_text(stmt, 5));
    auto refs_j   = nlohmann::json::parse(column_text(stmt, 6));
    auto payload_j = nlohmann::json::parse(column_text(stmt, 7));

    from_json(actor_j, e.actor);
    if (refs_j.is_array()) {
        e.refs = refs_j.get<std::vector<arcs::event::EventRef>>();
    }
    e.payload = payload_j;

    return e;
}

} // namespace

void StoreSqlite::commit(const CommitBundle& bundle)
{
    if (bundle.versions.empty()) {
        throw CommitRejectedError("commit rejected: no versions");
    }
    if (bundle.events.empty()) {
        throw CommitRejectedError("commit rejected: no events");
    }

    // Local consistency (duplicates within bundle).
    {
        std::unordered_set<std::string> vids;
        std::unordered_set<std::string> eids;
        for (const auto& pv : bundle.versions) {
            if (!vids.insert(pv.version.version_id).second) {
                throw CommitRejectedError(
                    "commit rejected: duplicate version_id inside bundle '" +
                    pv.version.version_id + "'");
            }
        }
        for (const auto& e : bundle.events) {
            if (!eids.insert(e.event_id).second) {
                throw CommitRejectedError(
                    "commit rejected: duplicate event_id inside bundle '" +
                    e.event_id + "'");
            }
        }
    }

    // Optimistic lock check BEFORE acquiring the write transaction.
    optimistic_lock::validate_bundle(bundle, *this);

    begin_immediate();
    try {
        // 1) Check global uniqueness against existing rows.
        for (const auto& pv : bundle.versions) {
            ensure_base_artifact_valid(pv.version);

            Stmt check = prepare_stmt(
                db_,
                "SELECT 1 FROM artifact_versions WHERE version_id = ?1;");
            bind_text(check.stmt, 1, pv.version.version_id);
            if (sqlite3_step(check.stmt) == SQLITE_ROW) {
                throw CommitRejectedError(
                    "artifact version rejected: duplicate version_id '" +
                    pv.version.version_id + "'");
            }
        }
        for (const auto& e : bundle.events) {
            Stmt check = prepare_stmt(
                db_,
                "SELECT 1 FROM events WHERE event_id = ?1;");
            bind_text(check.stmt, 1, e.event_id);
            if (sqlite3_step(check.stmt) == SQLITE_ROW) {
                throw CommitRejectedError(
                    "event rejected: duplicate event_id '" + e.event_id + "'");
            }
        }

        // 2) Insert all versions with monotonically increasing ordinals.
        for (const auto& pv : bundle.versions) {
            const auto& v = pv.version;
            const std::int64_t ord = next_ordinal();

            nlohmann::json created_by_j = v.created_by;
            nlohmann::json source_j    = v.source;
            nlohmann::json trust_j     = v.trust;
            nlohmann::json tags_j      = v.tags;
            nlohmann::json provenance_j = v.provenance;
            nlohmann::json payload_j   = v.payload;

            Stmt insert = prepare_stmt(
                db_,
                "INSERT INTO artifact_versions("
                "  version_id, artifact_id, version, type, schema_id, schema_version,"
                "  created_at, created_by_json, source_json, trust_json, stream_key,"
                "  tags_json, payload_json, provenance_json, ordinal"
                ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15);");

            bind_text(insert.stmt, 1, v.version_id);
            bind_text(insert.stmt, 2, v.artifact_id);
            bind_int64(insert.stmt, 3, v.version);
            bind_text(insert.stmt, 4, v.type);
            bind_text(insert.stmt, 5, v.schema_id);
            bind_int64(insert.stmt, 6, v.schema_version);
            bind_text(insert.stmt, 7, v.created_at);
            bind_text(insert.stmt, 8, created_by_j.dump());
            bind_text(insert.stmt, 9, source_j.dump());
            bind_text(insert.stmt, 10, trust_j.dump());
            bind_text(insert.stmt, 11, v.stream_key);
            bind_text(insert.stmt, 12, tags_j.dump());
            bind_text(insert.stmt, 13, payload_j.dump());
            bind_text(insert.stmt, 14, provenance_j.dump());
            bind_int64(insert.stmt, 15, ord);

            if (sqlite3_step(insert.stmt) != SQLITE_DONE) {
                throw_sqlite_error("step insert version in commit");
            }
        }

        // 3) Insert all events.
        for (const auto& e : bundle.events) {
            const std::int64_t ord = next_ordinal();

            nlohmann::json actor_j  = e.actor;
            nlohmann::json refs_j   = e.refs;
            nlohmann::json payload_j = e.payload;

            Stmt insert = prepare_stmt(
                db_,
                "INSERT INTO events("
                "  event_id, event_type, ts, actor_json, refs_json,"
                "  stream_key, payload_json, prev_hash, ordinal"
                ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);");

            bind_text(insert.stmt, 1, e.event_id);
            bind_text(insert.stmt, 2, e.event_type);
            bind_text(insert.stmt, 3, e.ts);
            bind_text(insert.stmt, 4, actor_j.dump());
            bind_text(insert.stmt, 5, refs_j.dump());
            bind_text(insert.stmt, 6, e.stream_key);
            bind_text(insert.stmt, 7, payload_j.dump());
            bind_text(insert.stmt, 8, e.prev_hash);
            bind_int64(insert.stmt, 9, ord);

            if (sqlite3_step(insert.stmt) != SQLITE_DONE) {
                throw_sqlite_error("step insert event in commit");
            }
        }

        // 4) Apply head_advanced events to artifact_heads, mirroring
        //    head_tracker semantics. We compute the projected heads in
        //    memory and then upsert the affected rows.
        {
            // Build current heads map.
            std::unordered_map<std::string, std::string> heads;
            {
                Stmt sel = prepare_stmt(
                    db_,
                    "SELECT artifact_id, current_head_version_id FROM artifact_heads;");
                while (sqlite3_step(sel.stmt) == SQLITE_ROW) {
                    heads[column_text(sel.stmt, 0)] = column_text(sel.stmt, 1);
                }
            }

            // Load versions referenced by the new events (only those in the bundle).
            std::unordered_map<std::string, ArtifactVersion> versions_by_version_id;
            for (const auto& pv : bundle.versions) {
                versions_by_version_id.emplace(pv.version.version_id, pv.version);
            }

            // Apply events through the shared head_tracker so the
            // semantics are identical to StoreMemory.
            head_tracker::apply_events(bundle.events, versions_by_version_id, heads);

            // Upsert changed heads.
            for (const auto& [artifact_id, head_version_id] : heads) {
                Stmt upsert = prepare_stmt(
                    db_,
                    "INSERT INTO artifact_heads(artifact_id, current_head_version_id) "
                    "VALUES (?1, ?2) "
                    "ON CONFLICT(artifact_id) DO UPDATE "
                    "  SET current_head_version_id = excluded.current_head_version_id;");
                bind_text(upsert.stmt, 1, artifact_id);
                bind_text(upsert.stmt, 2, head_version_id);
                if (sqlite3_step(upsert.stmt) != SQLITE_DONE) {
                    throw_sqlite_error("step upsert head");
                }
            }
        }

        commit_transaction();
    } catch (...) {
        rollback_transaction();
        throw;
    }
}

ArtifactVersion StoreSqlite::get(const std::string& artifact_id) const
{
    Stmt stmt = prepare_stmt(
        db_,
        "SELECT av.version_id, av.artifact_id, av.version, av.type, av.schema_id, "
        "       av.schema_version, av.created_at, av.stream_key, "
        "       av.created_by_json, av.source_json, av.trust_json, "
        "       av.tags_json, av.payload_json, av.provenance_json "
        "FROM artifact_heads h "
        "JOIN artifact_versions av ON av.version_id = h.current_head_version_id "
        "WHERE h.artifact_id = ?1;");
    bind_text(stmt.stmt, 1, artifact_id);
    if (sqlite3_step(stmt.stmt) != SQLITE_ROW) {
        throw NotFoundError(
            "store get failed: no head found for artifact_id '" + artifact_id + "'");
    }
    return load_artifact_version(stmt.stmt);
}

ArtifactVersion StoreSqlite::get_version(const std::string& version_id) const
{
    Stmt stmt = prepare_stmt(
        db_,
        "SELECT version_id, artifact_id, version, type, schema_id, "
        "       schema_version, created_at, stream_key, "
        "       created_by_json, source_json, trust_json, "
        "       tags_json, payload_json, provenance_json "
        "FROM artifact_versions WHERE version_id = ?1;");
    bind_text(stmt.stmt, 1, version_id);
    if (sqlite3_step(stmt.stmt) != SQLITE_ROW) {
        throw NotFoundError(
            "store get_version failed: unknown version_id '" + version_id + "'");
    }
    return load_artifact_version(stmt.stmt);
}

std::vector<ArtifactVersion> StoreSqlite::list(const ListQuery& query) const
{
    std::string sql =
        "SELECT version_id, artifact_id, version, type, schema_id, "
        "       schema_version, created_at, stream_key, "
        "       created_by_json, source_json, trust_json, "
        "       tags_json, payload_json, provenance_json "
        "FROM artifact_versions";

    std::vector<std::string> conditions;
    if (query.type.has_value()) {
        conditions.push_back("type = ?");
    }
    if (query.stream_key.has_value()) {
        conditions.push_back("stream_key = ?");
    }
    if (!conditions.empty()) {
        sql += " WHERE ";
        for (std::size_t i = 0; i < conditions.size(); ++i) {
            if (i > 0) {
                sql += " AND ";
            }
            sql += conditions[i];
        }
    }
    sql += " ORDER BY artifact_id, version_id;";

    Stmt stmt = prepare_stmt(db_, sql.c_str());

    int idx = 1;
    if (query.type.has_value()) {
        bind_text(stmt.stmt, idx++, *query.type);
    }
    if (query.stream_key.has_value()) {
        bind_text(stmt.stmt, idx++, *query.stream_key);
    }

    std::vector<ArtifactVersion> out;
    while (sqlite3_step(stmt.stmt) == SQLITE_ROW) {
        out.push_back(load_artifact_version(stmt.stmt));
    }
    return out;
}

std::vector<Event> StoreSqlite::list_events(
    const std::optional<std::string>& stream_key) const
{
    std::string sql =
        "SELECT event_id, event_type, ts, stream_key, prev_hash, "
        "       actor_json, refs_json, payload_json "
        "FROM events";
    if (stream_key.has_value()) {
        sql += " WHERE stream_key = ?";
    }
    sql += " ORDER BY ordinal;";

    Stmt stmt = prepare_stmt(db_, sql.c_str());
    if (stream_key.has_value()) {
        bind_text(stmt.stmt, 1, *stream_key);
    }

    std::vector<Event> out;
    while (sqlite3_step(stmt.stmt) == SQLITE_ROW) {
        out.push_back(load_event(stmt.stmt));
    }
    return out;
}

bool StoreSqlite::has_artifact(const std::string& artifact_id) const
{
    Stmt stmt = prepare_stmt(
        db_,
        "SELECT 1 FROM artifact_versions WHERE artifact_id = ?1 LIMIT 1;");
    bind_text(stmt.stmt, 1, artifact_id);
    return sqlite3_step(stmt.stmt) == SQLITE_ROW;
}

bool StoreSqlite::has_version(const std::string& version_id) const
{
    Stmt stmt = prepare_stmt(
        db_,
        "SELECT 1 FROM artifact_versions WHERE version_id = ?1 LIMIT 1;");
    bind_text(stmt.stmt, 1, version_id);
    return sqlite3_step(stmt.stmt) == SQLITE_ROW;
}

std::optional<std::string> StoreSqlite::current_head_version_id(
    const std::string& artifact_id) const
{
    Stmt stmt = prepare_stmt(
        db_,
        "SELECT current_head_version_id FROM artifact_heads WHERE artifact_id = ?1;");
    bind_text(stmt.stmt, 1, artifact_id);
    if (sqlite3_step(stmt.stmt) != SQLITE_ROW) {
        return std::nullopt;
    }
    return column_text(stmt.stmt, 0);
}

} // namespace arcs::store

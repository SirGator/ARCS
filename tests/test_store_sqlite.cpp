#include <gtest/gtest.h>

#include <cstdio>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "store/store_sqlite.hpp"

namespace {

using namespace arcs::store;
using arcs::artifact::ArtifactVersion;
using arcs::event::Event;
using arcs::event::EventRef;
using arcs::store::commit::CommitBundle;
using arcs::store::commit::PendingVersion;

class StoreSqliteTest : public ::testing::Test {
protected:
    void SetUp() override
    {
        // Unique temp file per test.
        const auto path = std::filesystem::temp_directory_path() /
            ("arcs_store_sqlite_" + std::to_string(::getpid()) + "_" +
             ::testing::UnitTest::GetInstance()->current_test_info()->name() + ".sqlite3");
        db_path_ = path.string();
    }

    void TearDown() override
    {
        std::error_code ec;
        std::filesystem::remove(db_path_, ec);
        std::filesystem::remove(db_path_ + "-wal", ec);
        std::filesystem::remove(db_path_ + "-shm", ec);
    }

    std::string db_path_;
};

ArtifactVersion make_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& type,
    const std::string& stream_key,
    int version = 1)
{
    ArtifactVersion a{};
    a.artifact_id = artifact_id;
    a.version_id = version_id;
    a.version = version;
    a.type = type;
    a.schema_id = "arcs." + type + ".v1";
    a.schema_version = 1;
    a.created_at = "2026-06-01T12:00:00Z";
    a.stream_key = stream_key;
    a.payload = nlohmann::json::object();
    if (type == "task") {
        a.payload = nlohmann::json{{"title", "Test task"}};
    }
    a.created_by.actor_type = "system";
    a.created_by.id = "test";
    a.source.kind = "internal";
    a.source.ref = "test";
    a.trust.level = "high";
    a.trust.source_class = "system";
    return a;
}

Event make_head_advanced_event(
    const std::string& event_id,
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& stream_key)
{
    Event e{};
    e.event_id = event_id;
    e.event_type = "head_advanced";
    e.stream_key = stream_key;
    e.actor.actor_type = "system";
    e.actor.id = "test";

    EventRef ref{};
    ref.artifact_id = artifact_id;
    ref.version_id = version_id;
    ref.role = "target";
    e.refs.push_back(ref);
    return e;
}

PendingVersion make_pending(
    const ArtifactVersion& version,
    const std::optional<std::string>& expected_head = std::nullopt)
{
    PendingVersion p{};
    p.version = version;
    p.expected_head_version_id = expected_head;
    return p;
}

TEST_F(StoreSqliteTest, OpenCreatesSchema)
{
    StoreSqlite store(db_path_);
    EXPECT_FALSE(db_path_.empty());

    // Force schema creation.
    ArtifactVersion v = make_artifact("a_t_1", "v_t_1", "task", "s_1");
    Event e = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(make_pending(v));
    b.events.push_back(e);
    store.commit(b);

    EXPECT_TRUE(store.has_artifact("a_t_1"));
    EXPECT_TRUE(store.has_version("v_t_1"));
}

TEST_F(StoreSqliteTest, CommitSingleVersionSetsHead)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1");
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(make_pending(v1));
    b.events.push_back(e1);
    store.commit(b);

    EXPECT_EQ(store.get("a_t_1").version_id, "v_t_1");
    auto head = store.current_head_version_id("a_t_1");
    ASSERT_TRUE(head.has_value());
    EXPECT_EQ(*head, "v_t_1");
}

TEST_F(StoreSqliteTest, CommitTwoVersionsAdvancesHead)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    ArtifactVersion v2 = make_artifact("a_t_1", "v_t_2", "task", "s_1", 2);
    Event e2 = make_head_advanced_event("e_2", "a_t_1", "v_t_2", "s_1");
    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2, std::string{"v_t_1"}));  // optimistic lock
    b2.events.push_back(e2);
    store.commit(b2);

    EXPECT_EQ(store.get("a_t_1").version_id, "v_t_2");
    EXPECT_EQ(*store.current_head_version_id("a_t_1"), "v_t_2");

    // list should return both versions.
    auto all = store.list({});
    EXPECT_EQ(all.size(), 2u);
}

TEST_F(StoreSqliteTest, OptimisticLockRejectsStaleHead)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    ArtifactVersion v2 = make_artifact("a_t_1", "v_t_2", "task", "s_1", 2);
    Event e2 = make_head_advanced_event("e_2", "a_t_1", "v_t_2", "s_1");
    CommitBundle b2{};
    // expected_head is v_t_1, but the actual head is v_t_1, so this works.
    b2.versions.push_back(make_pending(v2, std::string{"v_t_1"}));
    b2.events.push_back(e2);
    store.commit(b2);

    // Now try to commit v3 expecting v_t_1 as head — that must fail.
    ArtifactVersion v3 = make_artifact("a_t_1", "v_t_3", "task", "s_1", 3);
    Event e3 = make_head_advanced_event("e_3", "a_t_1", "v_t_3", "s_1");
    CommitBundle b3{};
    b3.versions.push_back(make_pending(v3, std::string{"v_t_1"}));
    b3.events.push_back(e3);
    EXPECT_THROW(store.commit(b3), CommitRejectedError);
}

TEST_F(StoreSqliteTest, CommitRejectsEmptyBundle)
{
    StoreSqlite store(db_path_);
    CommitBundle b{};
    EXPECT_THROW(store.commit(b), CommitRejectedError);
}

TEST_F(StoreSqliteTest, CommitRejectsDuplicateVersionId)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    // Second commit with a different artifact but same version_id must fail.
    ArtifactVersion v2 = make_artifact("a_t_2", "v_t_1", "task", "s_2", 1);
    Event e2 = make_head_advanced_event("e_2", "a_t_2", "v_t_1", "s_2");
    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2));
    b2.events.push_back(e2);
    EXPECT_THROW(store.commit(b2), CommitRejectedError);
}

TEST_F(StoreSqliteTest, CommitRejectsMissingSchemaId)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    v1.schema_id.clear();
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(make_pending(v1));
    b.events.push_back(e1);

    EXPECT_THROW(store.commit(b), CommitRejectedError);
}

TEST_F(StoreSqliteTest, CommitRejectsEmptyStreamKey)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(make_pending(v1));
    b.events.push_back(e1);

    EXPECT_THROW(store.commit(b), CommitRejectedError);
}

TEST_F(StoreSqliteTest, ListEventsInLogOrder)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    ArtifactVersion v2 = make_artifact("a_t_1", "v_t_2", "task", "s_1", 2);
    Event e2 = make_head_advanced_event("e_2", "a_t_1", "v_t_2", "s_1");
    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2, std::string{"v_t_1"}));
    b2.events.push_back(e2);
    store.commit(b2);

    auto events = store.list_events();
    ASSERT_EQ(events.size(), 2u);
    EXPECT_EQ(events[0].event_id, "e_1");
    EXPECT_EQ(events[1].event_id, "e_2");
}

TEST_F(StoreSqliteTest, ListEventsFilteredByStream)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    ArtifactVersion v2 = make_artifact("a_t_2", "v_t_2", "task", "s_2", 1);
    Event e2 = make_head_advanced_event("e_2", "a_t_2", "v_t_2", "s_2");
    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2));
    b2.events.push_back(e2);
    store.commit(b2);

    auto only_s1 = store.list_events(std::string{"s_1"});
    ASSERT_EQ(only_s1.size(), 1u);
    EXPECT_EQ(only_s1[0].stream_key, "s_1");
}

TEST_F(StoreSqliteTest, RoundTripJsonPayload)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    v.payload = nlohmann::json{
        {"title", "hello"},
        {"description", "payload roundtrip"},
        {"priority", "high"}
    };

    Event e = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(make_pending(v));
    b.events.push_back(e);
    store.commit(b);

    auto loaded = store.get("a_t_1");
    EXPECT_EQ(loaded.payload["title"], "hello");
    EXPECT_EQ(loaded.payload["description"], "payload roundtrip");
    EXPECT_EQ(loaded.payload["priority"], "high");
    EXPECT_EQ(loaded.schema_id, "arcs.task.v1");
}

TEST_F(StoreSqliteTest, PersistsAcrossReopen)
{
    {
        StoreSqlite store(db_path_);
        ArtifactVersion v = make_artifact("a_t_1", "v_t_1", "task", "s_1");
        Event e = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
        CommitBundle b{};
        b.versions.push_back(make_pending(v));
        b.events.push_back(e);
        store.commit(b);
    }

    // Reopen and verify data is still there.
    StoreSqlite reopened(db_path_);
    EXPECT_TRUE(reopened.has_artifact("a_t_1"));
    EXPECT_EQ(reopened.get("a_t_1").version_id, "v_t_1");
    auto events = reopened.list_events();
    ASSERT_EQ(events.size(), 1u);
    EXPECT_EQ(events[0].event_id, "e_1");
}

TEST_F(StoreSqliteTest, NonHeadAdvancedEventsDoNotChangeHead)
{
    StoreSqlite store(db_path_);

    ArtifactVersion v1 = make_artifact("a_t_1", "v_t_1", "task", "s_1", 1);
    Event e1 = make_head_advanced_event("e_1", "a_t_1", "v_t_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(e1);
    store.commit(b1);

    // A second event that is NOT head_advanced must not move the head.
    ArtifactVersion v2 = make_artifact("a_t_2", "v_t_2", "task", "s_2", 1);
    Event e2 = make_head_advanced_event("e_2", "a_t_2", "v_t_2", "s_2");
    // Override to a non-head_advanced event_type so head_tracker ignores it.
    e2.event_type = "task_created";

    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2));
    b2.events.push_back(e2);
    store.commit(b2);

    EXPECT_EQ(*store.current_head_version_id("a_t_1"), "v_t_1");
    // Since e2 is not head_advanced, head for a_t_2 is never set.
    EXPECT_FALSE(store.current_head_version_id("a_t_2").has_value());
}

TEST_F(StoreSqliteTest, GetUnknownArtifactThrows)
{
    StoreSqlite store(db_path_);
    EXPECT_THROW(store.get("does_not_exist"), NotFoundError);
}

} // namespace

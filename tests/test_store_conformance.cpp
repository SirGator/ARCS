#include <gtest/gtest.h>

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "store/store_memory.hpp"
#include "store/store_sqlite.hpp"

namespace {

using namespace arcs::store;
using arcs::artifact::ArtifactVersion;
using arcs::event::Event;
using arcs::event::EventRef;
using arcs::store::commit::CommitBundle;
using arcs::store::commit::PendingVersion;

// Type-parameterized conformance tests. Every test in this fixture is
// expected to pass against any IStore implementation. The point is to
// guarantee that no implementation accidentally drops a contract.
//
// Currently exercised:
//   - StoreMemory  (in-memory baseline)
//   - StoreSqlite  (persistent)

class IStoreConformance : public ::testing::TestWithParam<std::string> {
protected:
    void SetUp() override
    {
        if (GetParam() == "memory") {
            store_ = std::make_unique<StoreMemory>();
        } else if (GetParam() == "sqlite") {
            const auto info = ::testing::UnitTest::GetInstance()->current_test_info();
            // GoogleTest's test_suite_name() and name() (when using
            // INSTANTIATE_TEST_SUITE_P) may contain '/'. Sanitize every
            // component before joining into a filesystem path.
            std::string suite = info->test_suite_name();
            std::string test_name = info->name();
            std::replace(suite.begin(), suite.end(), '/', '_');
            std::replace(test_name.begin(), test_name.end(), '/', '_');
            const auto path = std::filesystem::temp_directory_path() /
                ("arcs_conformance_" + std::to_string(::getpid()) + "_" +
                 suite + "_" + test_name + ".sqlite3");
            db_path_ = path.string();
            store_ = std::make_unique<StoreSqlite>(db_path_);
        } else {
            FAIL() << "unknown store type in conformance test: " << GetParam();
        }
    }

    void TearDown() override
    {
        store_.reset();
        if (!db_path_.empty()) {
            std::error_code ec;
            std::filesystem::remove(db_path_, ec);
            std::filesystem::remove(db_path_ + "-wal", ec);
            std::filesystem::remove(db_path_ + "-shm", ec);
            db_path_.clear();
        }
    }

    std::unique_ptr<IStore> store_;
    std::string db_path_;
};

ArtifactVersion mk_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    int version,
    const std::string& type,
    const std::string& stream_key)
{
    ArtifactVersion a{};
    a.artifact_id = artifact_id;
    a.version_id = version_id;
    a.version = version;
    a.type = type;
    a.stream_key = stream_key;
    a.payload = nlohmann::json{{"i", version}};
    a.created_by.actor_type = "system";
    a.created_by.id = "test";
    a.source.kind = "internal";
    a.source.ref = "test";
    a.trust.level = "low";
    a.trust.source_class = "system";
    return a;
}

Event mk_head_advanced(
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

PendingVersion mk_pending(
    const ArtifactVersion& v,
    const std::optional<std::string>& expected = std::nullopt)
{
    PendingVersion p{};
    p.version = v;
    p.expected_head_version_id = expected;
    return p;
}

TEST_P(IStoreConformance, EmptyStoreHasNothing)
{
    EXPECT_FALSE(store_->has_artifact("a_1"));
    EXPECT_FALSE(store_->has_version("v_1"));
    EXPECT_FALSE(store_->current_head_version_id("a_1").has_value());
    EXPECT_TRUE(store_->list().empty());
    EXPECT_TRUE(store_->list_events().empty());
}

TEST_P(IStoreConformance, GetUnknownThrows)
{
    EXPECT_THROW(store_->get("missing"), NotFoundError);
    EXPECT_THROW(store_->get_version("missing"), NotFoundError);
}

TEST_P(IStoreConformance, CommitRejectsEmptyBundle)
{
    CommitBundle b{};
    EXPECT_THROW(store_->commit(b), CommitRejectedError);
}

TEST_P(IStoreConformance, CommitRejectsBundleWithoutEvents)
{
    CommitBundle b{};
    b.versions.push_back(mk_pending(mk_artifact("a_1", "v_1", 1, "task", "s_1")));
    EXPECT_THROW(store_->commit(b), CommitRejectedError);
}

TEST_P(IStoreConformance, SingleVersionSetsHead)
{
    auto v = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(mk_pending(v));
    b.events.push_back(e);
    store_->commit(b);

    EXPECT_EQ(store_->get("a_1").version_id, "v_1");
    EXPECT_EQ(*store_->current_head_version_id("a_1"), "v_1");
}

TEST_P(IStoreConformance, MultiVersionHeadAdvancesInEventOrder)
{
    auto v1 = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e1 = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(mk_pending(v1));
    b1.events.push_back(e1);
    store_->commit(b1);

    auto v2 = mk_artifact("a_1", "v_2", 2, "task", "s_1");
    auto e2 = mk_head_advanced("e_2", "a_1", "v_2", "s_1");
    CommitBundle b2{};
    b2.versions.push_back(mk_pending(v2, std::string{"v_1"}));
    b2.events.push_back(e2);
    store_->commit(b2);

    auto v3 = mk_artifact("a_1", "v_3", 3, "task", "s_1");
    auto e3 = mk_head_advanced("e_3", "a_1", "v_3", "s_1");
    CommitBundle b3{};
    b3.versions.push_back(mk_pending(v3, std::string{"v_2"}));
    b3.events.push_back(e3);
    store_->commit(b3);

    EXPECT_EQ(store_->get("a_1").version_id, "v_3");
    EXPECT_EQ(*store_->current_head_version_id("a_1"), "v_3");

    auto all = store_->list({});
    EXPECT_EQ(all.size(), 3u);
}

TEST_P(IStoreConformance, OptimisticLockRejectsStaleHead)
{
    auto v1 = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e1 = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(mk_pending(v1));
    b1.events.push_back(e1);
    store_->commit(b1);

    auto v2 = mk_artifact("a_1", "v_2", 2, "task", "s_1");
    auto e2 = mk_head_advanced("e_2", "a_1", "v_2", "s_1");
    CommitBundle b2{};
    b2.versions.push_back(mk_pending(v2, std::string{"v_1"}));
    b2.events.push_back(e2);
    store_->commit(b2);

    // Now v3 with stale head expectation.
    auto v3 = mk_artifact("a_1", "v_3", 3, "task", "s_1");
    auto e3 = mk_head_advanced("e_3", "a_1", "v_3", "s_1");
    CommitBundle b3{};
    b3.versions.push_back(mk_pending(v3, std::string{"v_1"}));  // wrong, head is v_2
    b3.events.push_back(e3);
    EXPECT_THROW(store_->commit(b3), CommitRejectedError);

    // And the state is unchanged.
    EXPECT_EQ(*store_->current_head_version_id("a_1"), "v_2");
}

TEST_P(IStoreConformance, DuplicateVersionIdInBundleRejected)
{
    auto v1 = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e1 = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    auto v2 = mk_artifact("a_2", "v_1", 1, "task", "s_2");  // same version_id
    auto e2 = mk_head_advanced("e_2", "a_2", "v_1", "s_2");
    CommitBundle b{};
    b.versions.push_back(mk_pending(v1));
    b.versions.push_back(mk_pending(v2));
    b.events.push_back(e1);
    b.events.push_back(e2);
    EXPECT_THROW(store_->commit(b), CommitRejectedError);
}

TEST_P(IStoreConformance, ListEventsInLogOrder)
{
    auto v1 = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e1 = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(mk_pending(v1));
    b1.events.push_back(e1);
    store_->commit(b1);

    auto v2 = mk_artifact("a_1", "v_2", 2, "task", "s_1");
    auto e2 = mk_head_advanced("e_2", "a_1", "v_2", "s_1");
    CommitBundle b2{};
    b2.versions.push_back(mk_pending(v2, std::string{"v_1"}));
    b2.events.push_back(e2);
    store_->commit(b2);

    auto events = store_->list_events();
    ASSERT_EQ(events.size(), 2u);
    EXPECT_EQ(events[0].event_id, "e_1");
    EXPECT_EQ(events[1].event_id, "e_2");
}

TEST_P(IStoreConformance, ListFilteredByType)
{
    auto t1 = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    auto e1 = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b1{};
    b1.versions.push_back(mk_pending(t1));
    b1.events.push_back(e1);
    store_->commit(b1);

    auto p1 = mk_artifact("a_2", "v_2", 1, "policy", "s_2");
    auto e2 = mk_head_advanced("e_2", "a_2", "v_2", "s_2");
    CommitBundle b2{};
    b2.versions.push_back(mk_pending(p1));
    b2.events.push_back(e2);
    store_->commit(b2);

    auto only_tasks = store_->list(ListQuery{std::string{"task"}, std::nullopt});
    EXPECT_EQ(only_tasks.size(), 1u);
    EXPECT_EQ(only_tasks[0].type, "task");
}

TEST_P(IStoreConformance, JsonPayloadRoundTrip)
{
    ArtifactVersion v{};
    v.artifact_id = "a_1";
    v.version_id = "v_1";
    v.version = 1;
    v.type = "task";
    v.schema_id = "arcs.task.v1";
    v.stream_key = "s_1";
    v.payload = nlohmann::json{
        {"text", "hello"},
        {"nested", {{"k", "v"}, {"n", 1}}},
        {"arr", nlohmann::json::array({1, 2, 3})}
    };
    v.created_by.actor_type = "system";
    v.created_by.id = "test";
    v.source.kind = "internal";
    v.source.ref = "test";
    v.trust.level = "low";
    v.trust.source_class = "system";
    auto e = mk_head_advanced("e_1", "a_1", "v_1", "s_1");
    CommitBundle b{};
    b.versions.push_back(mk_pending(v));
    b.events.push_back(e);
    store_->commit(b);

    auto loaded = store_->get("a_1");
    EXPECT_EQ(loaded.payload["text"], "hello");
    EXPECT_EQ(loaded.payload["nested"]["k"], "v");
    EXPECT_EQ(loaded.payload["arr"].size(), 3u);
    EXPECT_EQ(loaded.schema_id, "arcs.task.v1");
}

TEST_P(IStoreConformance, AppendArtifactBypassesCommitBoundary)
{
    // Debug-only path, must still reject duplicates and empties.
    ArtifactVersion v = mk_artifact("a_1", "v_1", 1, "task", "s_1");
    store_->append_artifact(v);
    EXPECT_TRUE(store_->has_version("v_1"));

    EXPECT_THROW(store_->append_artifact(v), CommitRejectedError);
}

INSTANTIATE_TEST_SUITE_P(
    AllStores,
    IStoreConformance,
    ::testing::Values("memory", "sqlite"),
    [](const ::testing::TestParamInfo<std::string>& info) {
        return info.param;
    });

} // namespace

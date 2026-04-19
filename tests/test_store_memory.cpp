#include <gtest/gtest.h>

#include "store/store_memory.hpp"

namespace arcs {
namespace {

using namespace arcs::store;

ArtifactVersion make_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& type,
    const std::string& stream_key)
{
    ArtifactVersion a{};
    a.artifact_id = artifact_id;
    a.version_id = version_id;
    a.type = type;
    a.stream_key = stream_key;
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

    arcs::event::EventRef ref{};
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

TEST(StoreMemoryTest, CommitSingleVersionAndHeadAdvancedSetsHead)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact(
        "a_task_1",
        "v_task_1",
        "task",
        "task_id:t_1");

    Event e1 = make_head_advanced_event(
        "e_1",
        "a_task_1",
        "v_task_1",
        "task_id:t_1");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));
    bundle.events.push_back(e1);

    store.commit(bundle);

    ArtifactVersion head = store.get("a_task_1");
    EXPECT_EQ(head.version_id, "v_task_1");
    EXPECT_EQ(head.artifact_id, "a_task_1");

    ArtifactVersion by_version = store.get_version("v_task_1");
    EXPECT_EQ(by_version.version_id, "v_task_1");
}

TEST(StoreMemoryTest, GetWithoutHeadThrows)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact(
        "a_task_1",
        "v_task_1",
        "task",
        "task_id:t_1");

    store.append_artifact(v1);

    EXPECT_THROW(store.get("a_task_1"), NotFoundError);
}

TEST(StoreMemoryTest, ListFiltersByTypeAndStreamKey)
{
    StoreMemory store;

    ArtifactVersion a1 = make_artifact("a1", "v1", "task",   "task_id:t_1");
    ArtifactVersion a2 = make_artifact("a2", "v2", "option", "task_id:t_1");
    ArtifactVersion a3 = make_artifact("a3", "v3", "task",   "task_id:t_2");

    store.append_artifact(a1);
    store.append_artifact(a2);
    store.append_artifact(a3);

    ListQuery q1{};
    q1.type = "task";

    auto tasks = store.list(q1);
    ASSERT_EQ(tasks.size(), 2u);

    ListQuery q2{};
    q2.stream_key = "task_id:t_1";

    auto t1_items = store.list(q2);
    ASSERT_EQ(t1_items.size(), 2u);

    ListQuery q3{};
    q3.type = "task";
    q3.stream_key = "task_id:t_1";

    auto filtered = store.list(q3);
    ASSERT_EQ(filtered.size(), 1u);
    EXPECT_EQ(filtered[0].artifact_id, "a1");
}

TEST(StoreMemoryTest, DuplicateVersionIdInsideBundleRejectsCommit)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v_dup", "task", "task_id:t_1");
    ArtifactVersion v2 = make_artifact("a2", "v_dup", "task", "task_id:t_2");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));
    bundle.versions.push_back(make_pending(v2));
    bundle.events.push_back(make_head_advanced_event("e1", "a1", "v_dup", "task_id:t_1"));

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, DuplicateEventIdInsideBundleRejectsCommit)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");

    Event e1 = make_head_advanced_event("e_dup", "a1", "v1", "task_id:t_1");
    Event e2 = make_head_advanced_event("e_dup", "a1", "v1", "task_id:t_1");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));
    bundle.events.push_back(e1);
    bundle.events.push_back(e2);

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, HeadAdvancedReferencingUnknownVersionRejectsCommit)
{
    StoreMemory store;

    Event e1 = make_head_advanced_event(
        "e_1",
        "a_task_1",
        "v_missing",
        "task_id:t_1");

    CommitBundle bundle{};
    bundle.events.push_back(e1);

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, HeadAdvancedWithWrongArtifactIdRejectsCommit)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));
    bundle.events.push_back(make_head_advanced_event("e1", "a_wrong", "v1", "task_id:t_1"));

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, AtomicCommitFailureDoesNotPartiallyWriteState)
{
    StoreMemory store;

    ArtifactVersion good = make_artifact("a1", "v1", "task", "task_id:t_1");

    CommitBundle bad_bundle{};
    bad_bundle.versions.push_back(make_pending(good));
    bad_bundle.events.push_back(
        make_head_advanced_event("e1", "a1", "v_missing", "task_id:t_1"));

    EXPECT_THROW(store.commit(bad_bundle), CommitRejectedError);

    EXPECT_FALSE(store.has_version("v1"));
    EXPECT_FALSE(store.has_artifact("a1"));
    EXPECT_THROW(store.get("a1"), NotFoundError);
    EXPECT_TRUE(store.list_events().empty());
}

TEST(StoreMemoryTest, CommitRejectsBundleWithoutEvents)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, CommitRejectsBundleWithoutVersions)
{
    StoreMemory store;

    CommitBundle bundle{};
    bundle.events.push_back(make_head_advanced_event("e1", "a1", "v1", "task_id:t_1"));

    EXPECT_THROW(store.commit(bundle), CommitRejectedError);
}

TEST(StoreMemoryTest, OptimisticLockPassesWhenExpectedHeadMatches)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");
    CommitBundle first{};
    first.versions.push_back(make_pending(v1));
    first.events.push_back(make_head_advanced_event("e1", "a1", "v1", "task_id:t_1"));
    store.commit(first);

    ArtifactVersion v2 = make_artifact("a1", "v2", "task", "task_id:t_1");
    CommitBundle second{};
    second.versions.push_back(make_pending(v2, "v1"));
    second.events.push_back(make_head_advanced_event("e2", "a1", "v2", "task_id:t_1"));

    EXPECT_NO_THROW(store.commit(second));

    ArtifactVersion head = store.get("a1");
    EXPECT_EQ(head.version_id, "v2");
}

TEST(StoreMemoryTest, OptimisticLockRejectsWhenExpectedHeadMismatch)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");
    CommitBundle first{};
    first.versions.push_back(make_pending(v1));
    first.events.push_back(make_head_advanced_event("e1", "a1", "v1", "task_id:t_1"));
    store.commit(first);

    ArtifactVersion v2 = make_artifact("a1", "v2", "task", "task_id:t_1");
    CommitBundle second{};
    second.versions.push_back(make_pending(v2, "wrong_head"));
    second.events.push_back(make_head_advanced_event("e2", "a1", "v2", "task_id:t_1"));

    EXPECT_THROW(store.commit(second), CommitRejectedError);

    ArtifactVersion head = store.get("a1");
    EXPECT_EQ(head.version_id, "v1");
    EXPECT_FALSE(store.has_version("v2"));
}

TEST(StoreMemoryTest, ListEventsCanFilterByStreamKey)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");
    ArtifactVersion v2 = make_artifact("a2", "v2", "task", "task_id:t_2");

    CommitBundle b1{};
    b1.versions.push_back(make_pending(v1));
    b1.events.push_back(make_head_advanced_event("e1", "a1", "v1", "task_id:t_1"));
    store.commit(b1);

    CommitBundle b2{};
    b2.versions.push_back(make_pending(v2));
    b2.events.push_back(make_head_advanced_event("e2", "a2", "v2", "task_id:t_2"));
    store.commit(b2);

    auto all_events = store.list_events();
    ASSERT_EQ(all_events.size(), 2u);

    auto t1_events = store.list_events("task_id:t_1");
    ASSERT_EQ(t1_events.size(), 1u);
    EXPECT_EQ(t1_events[0].event_id, "e1");
}

TEST(StoreMemoryTest, TwoHeadAdvancedEventsInSameCommitLastOneWins)
{
    StoreMemory store;

    ArtifactVersion v1 = make_artifact("a1", "v1", "task", "task_id:t_1");
    ArtifactVersion v2 = make_artifact("a1", "v2", "task", "task_id:t_1");

    CommitBundle bundle{};
    bundle.versions.push_back(make_pending(v1));
    bundle.versions.push_back(make_pending(v2));
    bundle.events.push_back(make_head_advanced_event("e1", "a1", "v1", "task_id:t_1"));
    bundle.events.push_back(make_head_advanced_event("e2", "a1", "v2", "task_id:t_1"));

    store.commit(bundle);

    ArtifactVersion head = store.get("a1");
    EXPECT_EQ(head.version_id, "v2");
}

} // namespace
} // namespace arcs

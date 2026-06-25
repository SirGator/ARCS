#include <gtest/gtest.h>

#include <atomic>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include "artifact/ids.hpp"

namespace {

using arcs::artifact::ids::new_artifact_id;
using arcs::artifact::ids::new_event_id;
using arcs::artifact::ids::new_version_id;

TEST(IdGeneratorTest, StartsWithKnownPrefix)
{
    EXPECT_EQ(new_artifact_id().substr(0, 2), "a_");
    EXPECT_EQ(new_version_id().substr(0, 2),  "v_");
    EXPECT_EQ(new_event_id().substr(0, 2),    "e_");
}

TEST(IdGeneratorTest, GeneratesNonEmptyIds)
{
    EXPECT_FALSE(new_artifact_id().empty());
    EXPECT_FALSE(new_version_id().empty());
    EXPECT_FALSE(new_event_id().empty());
}

TEST(IdGeneratorTest, ManyIdsAreUniqueWithinACall)
{
    constexpr int kCount = 10000;
    std::set<std::string> seen;
    for (int i = 0; i < kCount; ++i) {
        seen.insert(new_artifact_id());
        seen.insert(new_version_id());
        seen.insert(new_event_id());
    }
    EXPECT_EQ(seen.size(), static_cast<std::size_t>(kCount) * 3);
}

TEST(IdGeneratorTest, ConcurrentArtifactIdsAreUnique)
{
    constexpr int kThreads = 8;
    constexpr int kPerThread = 5000;

    std::vector<std::thread> threads;
    std::vector<std::vector<std::string>> per_thread(kThreads);

    for (int t = 0; t < kThreads; ++t) {
        threads.emplace_back([t, &per_thread]() {
            per_thread[t].reserve(kPerThread);
            for (int i = 0; i < kPerThread; ++i) {
                per_thread[t].push_back(new_artifact_id());
            }
        });
    }
    for (auto& th : threads) {
        th.join();
    }

    std::set<std::string> all;
    for (const auto& v : per_thread) {
        for (const auto& id : v) {
            all.insert(id);
        }
    }
    EXPECT_EQ(all.size(), static_cast<std::size_t>(kThreads) * kPerThread);
}

TEST(IdGeneratorTest, ConcurrentMixedIdsAreUnique)
{
    // Run all three generators in parallel and ensure no collision.
    constexpr int kThreads = 6;
    constexpr int kPerThread = 4000;

    std::vector<std::thread> threads;
    std::vector<std::vector<std::string>> per_thread(kThreads);

    for (int t = 0; t < kThreads; ++t) {
        threads.emplace_back([t, &per_thread]() {
            per_thread[t].reserve(kPerThread * 3);
            for (int i = 0; i < kPerThread; ++i) {
                per_thread[t].push_back(new_artifact_id());
                per_thread[t].push_back(new_version_id());
                per_thread[t].push_back(new_event_id());
            }
        });
    }
    for (auto& th : threads) {
        th.join();
    }

    std::set<std::string> all;
    for (const auto& v : per_thread) {
        for (const auto& id : v) {
            all.insert(id);
        }
    }
    EXPECT_EQ(all.size(), static_cast<std::size_t>(kThreads) * kPerThread * 3);
}

TEST(IdGeneratorTest, ConsecutiveCallsYieldDistinctIds)
{
    // Just a sanity test: two calls in a row on the same thread differ.
    auto a = new_artifact_id();
    auto b = new_artifact_id();
    EXPECT_NE(a, b);
}

} // namespace

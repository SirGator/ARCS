#include <gtest/gtest.h>

#include "core/commit/commit_service.hpp"

namespace {

arcs::artifact::ArtifactVersion make_artifact()
{
    arcs::artifact::ArtifactVersion artifact;
    artifact.artifact_id = "a_test";
    artifact.version_id = "v_test";
    artifact.type = "task";
    artifact.schema_id = "arcs.task.v1";
    artifact.created_at = "2026-07-05T12:00:00Z";
    artifact.created_by = {.actor_type = "system", .id = "kernel"};
    artifact.source = {.kind = "internal", .ref = "unit"};
    artifact.trust = {.level = "high", .source_class = "system"};
    artifact.stream_key = "task_id:t1";
    artifact.payload = nlohmann::json{{"title", "t"}, {"request", "r"}, {"requested_action", "report_emit"}};
    return artifact;
}

} // namespace

TEST(CommitServiceTest, AddsCommitIdAndStageToSemanticEvents)
{
    arcs::core::commit::CommitService service;
    const auto context = arcs::core::commit::CommitContext{
        .commit_id = "c_test_commit",
        .stage = "approval.requested",
        .correlation_id = "corr_123",
        .actor = "kernel",
        .reason = "unit test",
        .cause_refs = {arcs::event::EventRef{.artifact_id = "a_cause", .version_id = "v_cause", .role = "parent"}},
    };

    const auto bundle = service.make_bundle(context, "approval.requested", {make_artifact()}, "2026-07-05T12:00:00Z");

    ASSERT_EQ(bundle.events.size(), 2u);
    EXPECT_EQ(bundle.events[0].payload.value("commit_id", std::string{}), "c_test_commit");
    EXPECT_EQ(bundle.events[0].payload.value("stage", std::string{}), "approval.requested");
    EXPECT_EQ(bundle.events[0].payload.value("correlation_id", std::string{}), "corr_123");
    ASSERT_TRUE(bundle.events[0].payload.contains("cause_refs"));
    ASSERT_EQ(bundle.events[0].payload.at("cause_refs").size(), 1u);
    EXPECT_EQ(bundle.events[1].payload.value("commit_id", std::string{}), "c_test_commit");
    EXPECT_EQ(bundle.events[1].event_type, "head_advanced");
}

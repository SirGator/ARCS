#include <gtest/gtest.h>

#include <utility>

#include "artifact/artifact.hpp"
#include "reducer/task_state_reducer.hpp"

namespace arcs::reducer {
namespace {

arcs::artifact::ArtifactVersion make_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& type,
    nlohmann::json payload = nlohmann::json::object()) {
    arcs::artifact::ArtifactVersion artifact{};
    artifact.artifact_id = artifact_id;
    artifact.version_id = version_id;
    artifact.version = 1;
    artifact.type = type;
    artifact.schema_id = "test." + type + ".v1";
    artifact.schema_version = 1;
    artifact.created_at = "2026-01-01T00:00:00Z";
    artifact.created_by = arcs::artifact::ActorRef{"system", "test"};
    artifact.source = arcs::artifact::SourceRef{"internal", "test"};
    artifact.trust = arcs::artifact::TrustInfo{"high", "system"};
    artifact.stream_key = "task_id:t_test";
    artifact.tags = {};
    artifact.payload = std::move(payload);
    return artifact;
}

} // namespace

TEST(TaskStateReducerTest, ReturnsDraftWhenNoOptionExists) {
    TaskStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_task_1", "v_task_1", "task")
    };

    const TaskState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.status, "draft");
    EXPECT_TRUE(state.option_ids.empty());
    EXPECT_TRUE(state.approval_ids.empty());
}

TEST(TaskStateReducerTest, ReturnsBlockedWhenOptionExistsWithoutApproval) {
    TaskStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_task_1", "v_task_1", "task"),
        make_artifact("a_option_1", "v_option_1", "option")
    };

    const TaskState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.status, "blocked");
    ASSERT_EQ(state.option_ids.size(), 1u);
    EXPECT_EQ(state.option_ids[0], "a_option_1");
    EXPECT_TRUE(state.approval_ids.empty());
}

TEST(TaskStateReducerTest, ReturnsApprovedWhenOptionHasApproveDecision) {
    TaskStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_option_1", "v_option_1", "option"),
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"}
            })
    };

    const TaskState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.status, "approved");
    ASSERT_EQ(state.option_ids.size(), 1u);
    EXPECT_EQ(state.option_ids[0], "a_option_1");
    ASSERT_EQ(state.approval_ids.size(), 1u);
    EXPECT_EQ(state.approval_ids[0], "a_approval_1");
}

TEST(TaskStateReducerTest, RevokeAfterApproveReturnsBlocked) {
    TaskStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_option_1", "v_option_1", "option"),
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"}
            }),
        make_artifact(
            "a_approval_2",
            "v_approval_2",
            "approval",
            {
                {"decision", "revoke"}
            })
    };

    const TaskState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.status, "blocked");
    ASSERT_EQ(state.approval_ids.size(), 2u);
    EXPECT_EQ(state.approval_ids[0], "a_approval_1");
    EXPECT_EQ(state.approval_ids[1], "a_approval_2");
}

TEST(TaskStateReducerTest, ExecutionResultReturnsExecuted) {
    TaskStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_option_1", "v_option_1", "option"),
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"}
            }),
        make_artifact(
            "a_exec_1",
            "v_exec_1",
            "execution_result",
            {
                {"status", "success"}
            })
    };

    const TaskState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.status, "executed");
    ASSERT_EQ(state.option_ids.size(), 1u);
    EXPECT_EQ(state.option_ids[0], "a_option_1");
    ASSERT_EQ(state.approval_ids.size(), 1u);
    EXPECT_EQ(state.approval_ids[0], "a_approval_1");
}

} // namespace arcs::reducer

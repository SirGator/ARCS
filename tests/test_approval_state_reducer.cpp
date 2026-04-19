#include <gtest/gtest.h>

#include <utility>

#include "artifact/artifact.hpp"
#include "reducer/approval_state_reducer.hpp"

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

TEST(ApprovalStateReducerTest, ReturnsDefaultStateWhenNoApprovalExists) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact("a_task_1", "v_task_1", "task")
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_TRUE(state.decision.empty());
    EXPECT_TRUE(state.policy_ref.empty());
    EXPECT_FALSE(state.valid);
}

TEST(ApprovalStateReducerTest, ReadsApproveDecisionAndStringPolicyRef) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"},
                {"policy_ref", "v_policy_1"}
            })
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.decision, "approve");
    EXPECT_EQ(state.policy_ref, "v_policy_1");
    EXPECT_TRUE(state.valid);
}

TEST(ApprovalStateReducerTest, ReadsPolicyRefVersionIdFromObject) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"},
                {"policy_ref",
                 {
                     {"artifact_id", "a_policy_1"},
                     {"version_id", "v_policy_2"}
                 }}
            })
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.decision, "approve");
    EXPECT_EQ(state.policy_ref, "v_policy_2");
    EXPECT_TRUE(state.valid);
}

TEST(ApprovalStateReducerTest, FallsBackToPolicyArtifactIdWhenVersionIdMissing) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"},
                {"policy_ref",
                 {
                     {"artifact_id", "a_policy_9"}
                 }}
            })
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.decision, "approve");
    EXPECT_EQ(state.policy_ref, "a_policy_9");
    EXPECT_TRUE(state.valid);
}

TEST(ApprovalStateReducerTest, RejectDecisionIsNotValid) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "reject"},
                {"policy_ref", "v_policy_1"}
            })
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.decision, "reject");
    EXPECT_EQ(state.policy_ref, "v_policy_1");
    EXPECT_FALSE(state.valid);
}

TEST(ApprovalStateReducerTest, LastApprovalWins) {
    ApprovalStateReducer reducer;

    std::vector<arcs::artifact::ArtifactVersion> artifacts{
        make_artifact(
            "a_approval_1",
            "v_approval_1",
            "approval",
            {
                {"decision", "approve"},
                {"policy_ref", "v_policy_old"}
            }),
        make_artifact(
            "a_approval_2",
            "v_approval_2",
            "approval",
            {
                {"decision", "revoke"},
                {"policy_ref", "v_policy_new"}
            })
    };

    const ApprovalState state = reducer.reduce(artifacts);

    EXPECT_EQ(state.decision, "revoke");
    EXPECT_EQ(state.policy_ref, "v_policy_new");
    EXPECT_FALSE(state.valid);
}

} // namespace arcs::reducer

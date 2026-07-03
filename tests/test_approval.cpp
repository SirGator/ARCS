#include <gtest/gtest.h>

#include "approval.hpp"

using namespace arcs::approval;

namespace {

TEST(ApprovalGateTest, SubmitApproveBuildsApprovalArtifact) {
    ApprovalPayload in{};
    in.target_option = {"a_option_1", "v_option_1"};
    in.policy_ref    = {"a_policy_1", "v_policy_1"};
    in.verification_ref = {"a_report_1", "v_report_1"};
    in.request_ref = {"a_request_1", "v_request_1"};
    in.action_ref = {"a_action_1", "v_action_1"};
    in.decision      = ApprovalDecision::Approve;
    in.reason        = "checked";
    in.actor         = {"human", "user:simon"};
    in.timestamp     = "2026-02-09T10:00:00Z";
    in.expires_at    = "2026-02-09T11:00:00Z";
    in.approval_scope = "task_id:t_01";
    in.store_head_at_approval = "v_request_1";
    in.risk_summary = "safety_level=low; action_type=report_emit";

    ApprovalGate gate;
    auto approval = gate.submit(in);

    EXPECT_EQ(approval.type, "approval");
    EXPECT_EQ(approval.schema_id, "arcs.approval.v1");

    EXPECT_EQ(approval.payload["target_option"]["artifact_id"], "a_option_1");
    EXPECT_EQ(approval.payload["target_option"]["version_id"],  "v_option_1");

    EXPECT_EQ(approval.payload["policy_ref"]["artifact_id"], "a_policy_1");
    EXPECT_EQ(approval.payload["policy_ref"]["version_id"],  "v_policy_1");
    EXPECT_EQ(approval.payload["verification_ref"]["artifact_id"], "a_report_1");
    EXPECT_EQ(approval.payload["verification_ref"]["version_id"],  "v_report_1");
    EXPECT_EQ(approval.payload["request_ref"]["artifact_id"], "a_request_1");
    EXPECT_EQ(approval.payload["request_ref"]["version_id"],  "v_request_1");
    EXPECT_EQ(approval.payload["action_ref"]["artifact_id"], "a_action_1");
    EXPECT_EQ(approval.payload["action_ref"]["version_id"],  "v_action_1");

    EXPECT_EQ(approval.payload["decision"], "approve");
    EXPECT_EQ(approval.payload["reason"], "checked");
    EXPECT_EQ(approval.payload["actor"]["actor_type"], "human");
    EXPECT_EQ(approval.payload["actor"]["id"], "user:simon");
    EXPECT_EQ(approval.payload["timestamp"], "2026-02-09T10:00:00Z");
    EXPECT_EQ(approval.payload["expires_at"], "2026-02-09T11:00:00Z");
    EXPECT_EQ(approval.payload["approval_scope"], "task_id:t_01");
    EXPECT_EQ(approval.payload["store_head_at_approval"], "v_request_1");
    EXPECT_EQ(approval.payload["risk_summary"], "safety_level=low; action_type=report_emit");
}

TEST(ApprovalGateTest, SubmitRejectBuildsRejectDecision) {
    ApprovalPayload in{};
    in.target_option = {"a_option_1", "v_option_1"};
    in.policy_ref    = {"a_policy_1", "v_policy_1"};
    in.verification_ref = {"a_report_1", "v_report_1"};
    in.request_ref = {"a_request_1", "v_request_1"};
    in.action_ref = {"a_action_1", "v_action_1"};
    in.decision      = ApprovalDecision::Reject;
    in.reason        = "not safe";
    in.actor         = {"human", "user:simon"};
    in.timestamp     = "2026-02-09T10:00:00Z";
    in.expires_at    = "2026-02-09T11:00:00Z";
    in.approval_scope = "task_id:t_01";
    in.store_head_at_approval = "v_request_1";
    in.risk_summary = "safety_level=low; action_type=report_emit";

    ApprovalGate gate;
    auto approval = gate.submit(in);

    EXPECT_EQ(approval.payload["decision"], "reject");
    EXPECT_EQ(approval.payload["approval_scope"], "task_id:t_01");
}

} // namespace

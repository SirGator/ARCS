#include <gtest/gtest.h>

#include <string>

#include "approval.hpp"
#include "core/flow.hpp"

TEST(TextFlowTest, AllowsWhenApprovalAndPermissionAreYes)
{
    const auto output = arcs::core::run_text_flow(
        "request=generate_report",
        nullptr,
        {
            .enable_demo_controls = true,
            .demo_approval_granted = true,
            .demo_permission_granted = true,
        });

    EXPECT_NE(output.find("step: ingress_event -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: option -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("step: materialize action_candidate -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: approval_request -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: approval -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: promote action -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: action_verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("decision: not blocked"), std::string::npos);
    EXPECT_NE(output.find("demo approval and permission granted"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPermissionIsMissing)
{
    const auto output = arcs::core::run_text_flow(
        "request=generate_report",
        nullptr,
        {
            .enable_demo_controls = true,
            .demo_approval_granted = true,
        });

    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("permission: capability exec:report_emit fehlt"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenApprovalIsMissing)
{
    const auto output = arcs::core::run_text_flow(
        "request=generate_report",
        nullptr,
        {
            .enable_demo_controls = true,
            .demo_permission_granted = true,
        });

    EXPECT_NE(output.find("step: check approval -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("step: approval_request -> OK"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("reason: approval pending"), std::string::npos);
    EXPECT_NE(output.find("pending: approval_request"), std::string::npos);
    EXPECT_NE(output.find("resume: submit an approval artifact bound to approval_request"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPolicyDrifts)
{
    const auto output = arcs::core::run_text_flow(
        "request=generate_report",
        nullptr,
        {
            .enable_demo_controls = true,
            .demo_approval_granted = true,
            .demo_permission_granted = true,
            .demo_policy_drift = true,
        });

    EXPECT_NE(output.find("step: policy drift -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("policy_drift: option.policy_ref does not match current policy head"), std::string::npos);
}

TEST(TextFlowTest, BlocksFreeTextWithoutExternalApis)
{
    const auto output = arcs::core::run_text_flow("bitte erstelle einen bericht als json ueber die letzten pruefergebnisse");

    EXPECT_NE(output.find("step: parse input -> OK | free text routed through ingress and external interpretation artifact"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("reason: free text interpretation unavailable"), std::string::npos);
}

TEST(TextFlowTest, BlocksKeyValueAuthorityWhenDemoControlIsDisabled)
{
    const auto output = arcs::core::run_text_flow("approval=yes permission=yes");

    EXPECT_NE(output.find("step: parse input -> OK | demo control disabled | approval=no permission=no policy_drift=no | raw key-value input has no authority"), std::string::npos);
    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
}

TEST(TextFlowTest, DemoControlIgnoresKeyValueAuthorityInInput)
{
    const auto output = arcs::core::run_text_flow(
        "approval=yes permission=yes policy_drift=yes",
        nullptr,
        {.enable_demo_controls = true});

    EXPECT_NE(output.find("step: parse input -> OK | demo control parsed | approval=no permission=no policy_drift=no | raw key-value input has no authority"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
}

TEST(TextFlowTest, BlocksResumeWhenApprovalArtifactsAreMissingFromStore)
{
    arcs::approval::ApprovalPayload payload{};
    payload.target_option = {"a_option_resume", "v_option_resume"};
    payload.policy_ref = {"a_policy_core", "v_policy_002"};
    payload.verification_ref = {"a_resume_option_verification_report", "v_resume_option_verification_report"};
    payload.request_ref = {"a_request_resume", "v_request_resume"};
    payload.action_candidate_ref = {"a_candidate_resume", "v_candidate_resume"};
    payload.decision = arcs::approval::ApprovalDecision::Approve;
    payload.reason = "approved";
    payload.actor = {"human", "user:test"};
    payload.timestamp = "2026-07-04T14:00:00Z";
    payload.expires_at = "2026-07-04T23:00:00Z";
    payload.approval_scope = "task_id:t_resume";
    payload.store_head_at_approval = "v_candidate_resume";
    payload.risk_summary = "safety_level=low; action_type=report_emit";

    auto approval_artifact = arcs::approval::ApprovalGate{}.submit(payload);
    approval_artifact.stream_key = "task_id:t_resume";

    const auto output = arcs::core::run_text_flow(approval_artifact);

    EXPECT_NE(output.find("step: resume approval -> FAIL | resume artifacts missing in store"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("reason: resume artifacts missing in store"), std::string::npos);
}

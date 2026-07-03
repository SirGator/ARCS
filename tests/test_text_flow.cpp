#include <gtest/gtest.h>

#include <string>

#include "core/flow.hpp"

TEST(TextFlowTest, AllowsWhenApprovalAndPermissionAreYes)
{
    const auto output = arcs::core::run_text_flow(
        "approval=yes permission=yes",
        nullptr,
        {.enable_demo_controls = true});

    EXPECT_NE(output.find("step: ingress_event -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: option -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("step: approval_request -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: approval -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: action_verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("decision: not blocked"), std::string::npos);
    EXPECT_NE(output.find("demo approval and permission granted"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPermissionIsMissing)
{
    const auto output = arcs::core::run_text_flow(
        "approval=yes permission=no",
        nullptr,
        {.enable_demo_controls = true});

    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("permission: capability exec:report_emit fehlt"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenApprovalIsMissing)
{
    const auto output = arcs::core::run_text_flow(
        "approval=no permission=yes",
        nullptr,
        {.enable_demo_controls = true});

    EXPECT_NE(output.find("step: check approval -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("approval: missing approval"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPolicyDrifts)
{
    const auto output = arcs::core::run_text_flow(
        "approval=yes permission=yes policy_drift=yes",
        nullptr,
        {.enable_demo_controls = true});

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

    EXPECT_NE(output.find("step: parse input -> OK | demo control disabled | approval=no permission=no policy_drift=no"), std::string::npos);
    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
}

#include <gtest/gtest.h>

#include <string>

#include "core/flow.hpp"

TEST(TextFlowTest, AllowsWhenApprovalAndPermissionAreYes)
{
    const auto output = arcs::core::run_text_flow("approval=yes permission=yes");

    EXPECT_NE(output.find("step: ingress_event -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: option -> OK"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> OK | pass"), std::string::npos);
    EXPECT_NE(output.find("step: approval -> OK"), std::string::npos);
    EXPECT_NE(output.find("decision: not blocked"), std::string::npos);
    EXPECT_NE(output.find("approval=yes and permission=yes"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPermissionIsMissing)
{
    const auto output = arcs::core::run_text_flow("approval=yes permission=no");

    EXPECT_NE(output.find("step: check permission -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("permission: capability exec:report_emit fehlt"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenApprovalIsMissing)
{
    const auto output = arcs::core::run_text_flow("approval=no permission=yes");

    EXPECT_NE(output.find("step: check approval -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("approval: missing approval"), std::string::npos);
}

TEST(TextFlowTest, BlocksWhenPolicyDrifts)
{
    const auto output = arcs::core::run_text_flow("approval=yes permission=yes policy_drift=yes");

    EXPECT_NE(output.find("step: policy drift -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("step: verification_report -> FAIL"), std::string::npos);
    EXPECT_NE(output.find("decision: blocked"), std::string::npos);
    EXPECT_NE(output.find("policy_drift: option.policy_ref does not match current policy head"), std::string::npos);
}

TEST(TextFlowTest, RoutesFreeTextThroughInterpretation)
{
    const auto output = arcs::core::run_text_flow("bitte erstelle einen bericht als json ueber die letzten pruefergebnisse");

    EXPECT_NE(output.find("step: parse input -> OK | free text routed to interpretation"), std::string::npos);
    EXPECT_NE(output.find("step: interpretation_proposal -> OK | status=parsed"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK | artifact created"), std::string::npos);
    EXPECT_NE(output.find("step: routing -> OK | task-only ingestion"), std::string::npos);
    EXPECT_NE(output.find("decision: ingested"), std::string::npos);
    EXPECT_EQ(output.find("step: option -> OK"), std::string::npos);
}

TEST(TextFlowTest, SkipsTaskWhenInterpretationIsUnknown)
{
    const auto output = arcs::core::run_text_flow("termin morgen 18 uhr");

    EXPECT_NE(output.find("step: interpretation_proposal -> OK | status=unknown"), std::string::npos);
    EXPECT_NE(output.find("step: task -> OK | skipped | no safe task mapping"), std::string::npos);
    EXPECT_NE(output.find("step: routing -> OK | interpretation-only ingestion"), std::string::npos);
    EXPECT_NE(output.find("reason: interpretation unknown; no task created"), std::string::npos);
    EXPECT_EQ(output.find("step: task -> OK | artifact created"), std::string::npos);
}

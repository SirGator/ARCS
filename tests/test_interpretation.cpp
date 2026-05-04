#include <gtest/gtest.h>

#include <string>

#include <nlohmann/json.hpp>

#include "interpretation/api_interpreter.hpp"
#include "interpretation/interpretation_proposal.hpp"
#include "interpretation/interpretation_to_task_mapper.hpp"
#include "interpretation/rule_based_interpreter.hpp"

TEST(InterpretationTest, RuleBasedInterpreterParsesReportRequests)
{
    arcs::interpretation::RuleBasedInterpreter interpreter;

    const auto proposal = interpreter.interpret("Bitte erstelle einen Bericht als JSON ueber die letzten Pruefergebnisse.");

    EXPECT_EQ(proposal.status, arcs::interpretation::InterpretationStatus::Parsed);
    EXPECT_EQ(proposal.intent, "create_report");
    EXPECT_EQ(proposal.format, "json");
    EXPECT_EQ(proposal.target, "Bitte erstelle einen Bericht als JSON ueber die letzten Pruefergebnisse.");
    EXPECT_NEAR(proposal.confidence, 0.65, 1e-9);
}

TEST(InterpretationTest, RuleBasedInterpreterMarksUnknownInput)
{
    arcs::interpretation::RuleBasedInterpreter interpreter;

    const auto proposal = interpreter.interpret("Hallo Welt");

    EXPECT_EQ(proposal.status, arcs::interpretation::InterpretationStatus::Unknown);
    EXPECT_EQ(proposal.intent, "unknown");
    EXPECT_EQ(proposal.format, "text");
    EXPECT_EQ(proposal.target, "Hallo Welt");
    EXPECT_NEAR(proposal.confidence, 0.20, 1e-9);
}

TEST(InterpretationTest, MapperCreatesTaskDraftForSupportedProposal)
{
    arcs::interpretation::InterpretationProposal proposal;
    proposal.status = arcs::interpretation::InterpretationStatus::Parsed;
    proposal.intent = "create_report";
    proposal.target = "letzte Pruefergebnisse";
    proposal.format = "json";
    proposal.confidence = 0.75;
    proposal.raw_text = "Erstelle einen Bericht";

    const auto task = arcs::interpretation::InterpretationToTaskMapper{}.map(proposal);

    ASSERT_TRUE(task.has_value());
    EXPECT_EQ(task->intent, "create_report");
    EXPECT_EQ(task->target, "letzte Pruefergebnisse");
    EXPECT_EQ(task->format, "json");
    EXPECT_EQ(task->source_text, "Erstelle einen Bericht");
    EXPECT_EQ(task->title, "Interpret user request: create_report -> letzte Pruefergebnisse");
}

TEST(InterpretationTest, MapperRejectsLowConfidenceOrFailedProposals)
{
    arcs::interpretation::InterpretationProposal proposal;
    proposal.status = arcs::interpretation::InterpretationStatus::Parsed;
    proposal.intent = "create_report";
    proposal.target = "letzte Pruefergebnisse";
    proposal.format = "json";
    proposal.confidence = 0.49;

    EXPECT_FALSE(arcs::interpretation::InterpretationToTaskMapper{}.map(proposal).has_value());

    proposal.confidence = 0.75;
    proposal.status = arcs::interpretation::InterpretationStatus::Failed;

    EXPECT_FALSE(arcs::interpretation::InterpretationToTaskMapper{}.map(proposal).has_value());
}

TEST(InterpretationTest, ApiInterpreterFailsWithoutEndpoint)
{
    arcs::interpretation::ApiInterpreter interpreter("");

    const auto proposal = interpreter.interpret("anything");

    EXPECT_EQ(proposal.status, arcs::interpretation::InterpretationStatus::Failed);
    EXPECT_EQ(proposal.intent, "unknown");
    EXPECT_EQ(proposal.raw_text, "anything");
    EXPECT_NE(proposal.reason.find("missing interpretation API endpoint URL"), std::string::npos);
}

TEST(InterpretationTest, ParsesOpenAiChatCompletionResponse)
{
    const nlohmann::json response = {
        {"choices", nlohmann::json::array({
            {
                {"message", {
                    {"content", R"({"status":"parsed","intent":"create_report","target":"weekly summary","format":"json","confidence":0.92,"reason":"structured output"})"}
                }}
            }
        })}
    };

    const auto proposal = arcs::interpretation::interpretation_proposal_from_response("weekly summary please", response);

    EXPECT_EQ(proposal.status, arcs::interpretation::InterpretationStatus::Parsed);
    EXPECT_EQ(proposal.intent, "create_report");
    EXPECT_EQ(proposal.target, "weekly summary");
    EXPECT_EQ(proposal.format, "json");
    EXPECT_NEAR(proposal.confidence, 0.92, 1e-9);
    EXPECT_EQ(proposal.reason, "structured output");
}

TEST(InterpretationTest, StatusStringRoundTrips)
{
    EXPECT_EQ(arcs::interpretation::to_string(arcs::interpretation::InterpretationStatus::Parsed), "parsed");
    EXPECT_EQ(arcs::interpretation::interpretation_status_from_string("failed"), arcs::interpretation::InterpretationStatus::Failed);
    EXPECT_EQ(arcs::interpretation::interpretation_status_from_string("something-else"), arcs::interpretation::InterpretationStatus::Unknown);
}

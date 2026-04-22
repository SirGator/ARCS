#include <gtest/gtest.h>

#include "materializer.hpp"
#include "step.hpp"
#include "artifact/artifact.hpp"

using arcs::artifact::ArtifactVersion;
using namespace arcs::execution;

namespace {

TEST(ActionMaterializerTest, EmitReportStepBecomesReportEmitAction) {
    ArtifactVersion option{};
    option.type = "option";
    option.schema_id = "arcs.option.v1";
    option.stream_key = "task_id:t_01";

    option.payload = {
        {"steps", {{
            {"kind", "emit_report"},
            {"params", {
                {"format", "pdf"},
                {"sections", {"summary", "risks"}}
            }}
        }}}
    };

    ArtifactVersion policy{};
    policy.type = "policy";
    policy.schema_id = "arcs.policy.v1";
    policy.payload = {
        {"capabilities", {"exec:report_emit"}},
        {"approval_required_for", {"exec:report_emit"}}
    };

    ActionMaterializer materializer;
    auto actions = materializer.materialize(option, policy);

    ASSERT_EQ(actions.size(), 1u);
    EXPECT_EQ(actions[0].type, "action");
    EXPECT_EQ(actions[0].schema_id, "arcs.action.v1");

    EXPECT_EQ(actions[0].payload["type"], "report_emit");
    EXPECT_EQ(actions[0].payload["params"]["format"], "pdf");
    EXPECT_EQ(actions[0].payload["params"]["sections"][0], "summary");
    EXPECT_EQ(actions[0].payload["params"]["sections"][1], "risks");
    EXPECT_EQ(actions[0].payload["required_permissions"][0], "exec:report_emit");
    EXPECT_FALSE(actions[0].payload["idempotency_key"].get<std::string>().empty());
}

TEST(ActionMaterializerTest, UnsupportedStepThrows) {
    ArtifactVersion option{};
    option.type = "option";
    option.payload = {
        {"steps", {{
            {"kind", "unknown_step"},
            {"params", {{"x", 1}}}
        }}}
    };

    ArtifactVersion policy{};
    policy.type = "policy";

    ActionMaterializer materializer;
    EXPECT_THROW(materializer.materialize(option, policy), std::runtime_error);
}

TEST(ActionMaterializerTest, MaterializeIsDeterministic) {
    ArtifactVersion option{};
    option.type = "option";
    option.stream_key = "task_id:t_01";
    option.payload = {
        {"steps", {{
            {"kind", "emit_report"},
            {"params", {
                {"format", "json"},
                {"sections", {"summary"}}
            }}
        }}}
    };

    ArtifactVersion policy{};
    policy.type = "policy";
    policy.payload = {
        {"capabilities", {"exec:report_emit"}}
    };

    ActionMaterializer materializer;
    auto a1 = materializer.materialize(option, policy);
    auto a2 = materializer.materialize(option, policy);

    ASSERT_EQ(a1.size(), a2.size());
    EXPECT_EQ(a1[0].payload["type"], a2[0].payload["type"]);
    EXPECT_EQ(a1[0].payload["params"], a2[0].payload["params"]);
    EXPECT_EQ(a1[0].payload["required_permissions"], a2[0].payload["required_permissions"]);
}

} // namespace

#include <gtest/gtest.h>

#include <string>

#include <nlohmann/json.hpp>

#include "approval.hpp"
#include "artifact/json.hpp"
#include "core/runtime/core_request.hpp"
#include "core/runtime/runtime_context.hpp"
#include "core/runtime/core_runtime.hpp"
#include "core/runtime/runtime_support.hpp"
#include "store/store_memory.hpp"

namespace {

bool has_stage(const arcs::core::FlowResult& result, const std::string& stage, const arcs::core::DiagnosticSeverity severity)
{
    for (const auto& diagnostic : result.diagnostics) {
        if (diagnostic.stage == stage && diagnostic.severity == severity) {
            return true;
        }
    }

    return false;
}

nlohmann::json artifact_by_type(const arcs::store::IStore& store, const std::string& type)
{
    const auto artifacts = store.list(arcs::store::ListQuery{.type = type});
    if (!artifacts.empty()) {
        return nlohmann::json(artifacts.back());
    }

    ADD_FAILURE() << "artifact type not found: " << type;
    return nlohmann::json::object();
}

} // namespace

TEST(CoreRuntimePersistenceTest, ReusesStoreAcrossRunsForApprovalResume)
{
    arcs::store::StoreMemory store;
    arcs::core::runtime::CoreRuntime runtime(store, arcs::core::runtime::default_payload_schema_registry());

    const auto first = runtime.run(arcs::core::runtime::CoreRequest{
        .input = "request=generate_report",
        .interpretation_config = nullptr,
        .options = {
            .enable_demo_controls = true,
            .demo_permission_granted = true,
        },
    });

    EXPECT_EQ(first.status, arcs::core::FlowStatus::Pending);
    EXPECT_EQ(first.reason, "approval pending");
    ASSERT_TRUE(first.pending.has_value());
    EXPECT_EQ(first.pending->kind, "approval_request");

    const auto verification_report = artifact_by_type(store, "verification_report");
    const auto action_candidate = artifact_by_type(store, "action_candidate");
    const auto approval_request = artifact_by_type(store, "approval_request");
    const auto option_ref = action_candidate.at("payload").at("option_ref");
    const auto policy_ref = action_candidate.at("payload").at("policy_ref");

    arcs::approval::ApprovalPayload payload{};
    payload.target_option = {
        option_ref.at("artifact_id").get<std::string>(),
        option_ref.at("version_id").get<std::string>(),
    };
    payload.policy_ref = {
        policy_ref.at("artifact_id").get<std::string>(),
        policy_ref.at("version_id").get<std::string>(),
    };
    payload.verification_ref = {
        verification_report.at("artifact_id").get<std::string>(),
        verification_report.at("version_id").get<std::string>(),
    };
    payload.request_ref = {
        approval_request.at("artifact_id").get<std::string>(),
        approval_request.at("version_id").get<std::string>(),
    };
    payload.action_candidate_ref = {
        action_candidate.at("artifact_id").get<std::string>(),
        action_candidate.at("version_id").get<std::string>(),
    };
    payload.decision = arcs::approval::ApprovalDecision::Approve;
    payload.reason = "approved";
    payload.actor = {"human", "user:test"};
    payload.timestamp = arcs::core::runtime::utc_now();
    payload.expires_at = arcs::core::runtime::utc_after_hours(1);
    payload.approval_scope = action_candidate.at("stream_key").get<std::string>();
    payload.store_head_at_approval = action_candidate.at("version_id").get<std::string>();
    payload.risk_summary = "safety_level=low; action_type=report_emit";

    auto approval_artifact = arcs::approval::ApprovalGate{}.submit(payload);
    approval_artifact.stream_key = action_candidate.at("stream_key").get<std::string>();

    const auto resumed = runtime.run(approval_artifact, {});

    EXPECT_EQ(resumed.status, arcs::core::FlowStatus::Completed);
    EXPECT_EQ(resumed.reason, "approval resumed and action executed");
    EXPECT_TRUE(has_stage(resumed, "resume approval", arcs::core::DiagnosticSeverity::Info));
}

#include <gtest/gtest.h>

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "reducer/mock_time_source.hpp"
#include "store/store_memory.hpp"
#include "verification/verifier.hpp"

namespace {

using arcs::artifact::ArtifactVersion;

ArtifactVersion make_action()
{
    ArtifactVersion action{};
    action.artifact_id = "a_exec_act_1";
    action.version_id = "v_act_1_action";
    action.version = 1;
    action.type = "action";
    action.schema_id = "arcs.action.report_emit.v1";
    action.schema_version = 1;
    action.created_at = "2026-07-03T09:00:00Z";
    action.created_by = {"system", "kernel"};
    action.source = {"internal", "test"};
    action.trust = {"high", "system"};
    action.stream_key = "task_id:t_01";
    action.payload = {
        {"action_id", "act_1"},
        {"type", "report_emit"},
        {"option_ref", {{"artifact_id", "a_option_1"}, {"version_id", "v_option_1"}}},
        {"policy_ref", {{"artifact_id", "a_policy_1"}, {"version_id", "v_policy_1"}}},
        {"action_candidate_ref", {{"artifact_id", "a_candidate_1"}, {"version_id", "v_candidate_1"}}},
        {"verification_ref", {{"artifact_id", "a_report_1"}, {"version_id", "v_report_1"}}},
        {"approval_ref", {{"artifact_id", "a_approval_1"}, {"version_id", "v_approval_1"}}},
        {"params", {{"format", "json"}, {"sections", {"summary"}}}},
        {"required_permissions", {"exec:report_emit"}},
        {"safety_level", "low"},
        {"idempotency_key", "act_1"},
    };
    return action;
}

ArtifactVersion make_policy()
{
    ArtifactVersion policy{};
    policy.artifact_id = "a_policy_1";
    policy.version_id = "v_policy_1";
    policy.version = 1;
    policy.type = "policy";
    policy.schema_id = "arcs.policy.v1";
    policy.schema_version = 1;
    policy.created_at = "2026-07-03T09:00:00Z";
    policy.created_by = {"system", "kernel"};
    policy.source = {"internal", "test"};
    policy.trust = {"high", "system"};
    return policy;
}

ArtifactVersion make_approval(const ArtifactVersion& action, const std::string& expires_at)
{
    arcs::approval::ApprovalPayload payload{};
    payload.target_option = {"a_option_1", "v_option_1"};
    payload.policy_ref = {"a_policy_1", "v_policy_1"};
    payload.verification_ref = {"a_report_1", "v_report_1"};
    payload.request_ref = {"a_request_1", "v_request_1"};
    payload.action_candidate_ref = {"a_candidate_1", "v_candidate_1"};
    payload.decision = arcs::approval::ApprovalDecision::Approve;
    payload.reason = "checked";
    payload.actor = {"human", "user:test"};
    payload.timestamp = "2026-07-03T09:00:00Z";
    payload.expires_at = expires_at;
    payload.approval_scope = action.stream_key;
    payload.store_head_at_approval = action.version_id;
    payload.risk_summary = "safety_level=low; action_type=report_emit";

    auto approval = arcs::approval::ApprovalGate{}.submit(payload);
    approval.artifact_id = "a_approval_1";
    approval.version_id = "v_approval_1";
    approval.stream_key = action.stream_key;
    return approval;
}

TEST(ApprovalVerifierTest, PassesWhenMatchingApprovalExists)
{
    arcs::store::StoreMemory store;
    const auto action = make_action();
    const auto approval = make_approval(action, "2026-07-03T10:00:00Z");

    arcs::store::CommitBundle bundle{};
    bundle.versions.push_back({approval, std::nullopt});
    bundle.events.push_back(arcs::event::Event{
        .event_id = "e_approval_1",
        .event_type = "head_advanced",
        .ts = "2026-07-03T09:00:00Z",
        .actor = approval.created_by,
        .refs = {{approval.artifact_id, approval.version_id, "target"}},
        .stream_key = approval.stream_key,
        .payload = {{"artifact_type", approval.type}, {"schema_id", approval.schema_id}},
    });
    store.commit(bundle);

    arcs::reducer::MockTimeSource time_source("2026-07-03T09:30:00Z");
    arcs::verification::VerificationContext context{};
    const auto policy = make_policy();
    context.policy = &policy;
    context.store = &store;
    context.time_source = &time_source;

    const auto check = arcs::verification::ApprovalVerifier{}.check(action, context);
    EXPECT_EQ(check.status, arcs::verification::CheckStatus::Pass);
}

TEST(ApprovalVerifierTest, FailsWhenApprovalExpired)
{
    arcs::store::StoreMemory store;
    const auto action = make_action();
    const auto approval = make_approval(action, "2026-07-03T09:10:00Z");

    arcs::store::CommitBundle bundle{};
    bundle.versions.push_back({approval, std::nullopt});
    bundle.events.push_back(arcs::event::Event{
        .event_id = "e_approval_2",
        .event_type = "head_advanced",
        .ts = "2026-07-03T09:00:00Z",
        .actor = approval.created_by,
        .refs = {{approval.artifact_id, approval.version_id, "target"}},
        .stream_key = approval.stream_key,
        .payload = {{"artifact_type", approval.type}, {"schema_id", approval.schema_id}},
    });
    store.commit(bundle);

    arcs::reducer::MockTimeSource time_source("2026-07-03T09:30:00Z");
    arcs::verification::VerificationContext context{};
    const auto policy = make_policy();
    context.policy = &policy;
    context.store = &store;
    context.time_source = &time_source;

    const auto check = arcs::verification::ApprovalVerifier{}.check(action, context);
    EXPECT_EQ(check.status, arcs::verification::CheckStatus::Fail);
    EXPECT_EQ(check.detail, "approval expired");
}

TEST(ApprovalVerifierTest, FailsWhenApprovalVerificationBindingDiffers)
{
    arcs::store::StoreMemory store;
    auto action = make_action();
    action.payload["verification_ref"] = {
        {"artifact_id", "a_report_other"},
        {"version_id", "v_report_other"},
    };
    const auto approval = make_approval(action, "2026-07-03T10:00:00Z");

    arcs::store::CommitBundle bundle{};
    bundle.versions.push_back({approval, std::nullopt});
    bundle.events.push_back(arcs::event::Event{
        .event_id = "e_approval_3",
        .event_type = "head_advanced",
        .ts = "2026-07-03T09:00:00Z",
        .actor = approval.created_by,
        .refs = {{approval.artifact_id, approval.version_id, "target"}},
        .stream_key = approval.stream_key,
        .payload = {{"artifact_type", approval.type}, {"schema_id", approval.schema_id}},
    });
    store.commit(bundle);

    arcs::reducer::MockTimeSource time_source("2026-07-03T09:30:00Z");
    arcs::verification::VerificationContext context{};
    const auto policy = make_policy();
    context.policy = &policy;
    context.store = &store;
    context.time_source = &time_source;

    const auto check = arcs::verification::ApprovalVerifier{}.check(action, context);
    EXPECT_EQ(check.status, arcs::verification::CheckStatus::Fail);
    EXPECT_EQ(check.detail, "approval verification_ref does not match action");
}

TEST(ApprovalVerifierTest, FailsWhenApprovalStoreHeadIsStale)
{
    arcs::store::StoreMemory store;
    const auto action = make_action();
    auto approval = make_approval(action, "2026-07-03T10:00:00Z");
    approval.payload["store_head_at_approval"] = "v_candidate_old";

    ArtifactVersion candidate = action;
    candidate.artifact_id = "a_candidate_1";
    candidate.version_id = "v_candidate_new";
    candidate.type = "action_candidate";
    candidate.schema_id = "arcs.action_candidate.report_emit.v1";
    candidate.payload.erase("action_candidate_ref");
    candidate.payload.erase("approval_ref");
    candidate.payload.erase("verification_ref");

    arcs::store::CommitBundle bundle{};
    bundle.versions.push_back({candidate, std::nullopt});
    bundle.events.push_back(arcs::event::Event{
        .event_id = "e_candidate_1",
        .event_type = "head_advanced",
        .ts = "2026-07-03T08:55:00Z",
        .actor = action.created_by,
        .refs = {{candidate.artifact_id, candidate.version_id, "target"}},
        .stream_key = candidate.stream_key,
        .payload = {{"artifact_type", candidate.type}, {"schema_id", candidate.schema_id}},
    });
    bundle.versions.push_back({approval, std::nullopt});
    bundle.events.push_back(arcs::event::Event{
        .event_id = "e_approval_4",
        .event_type = "head_advanced",
        .ts = "2026-07-03T09:00:00Z",
        .actor = approval.created_by,
        .refs = {{approval.artifact_id, approval.version_id, "target"}},
        .stream_key = approval.stream_key,
        .payload = {{"artifact_type", approval.type}, {"schema_id", approval.schema_id}},
    });
    store.commit(bundle);

    arcs::reducer::MockTimeSource time_source("2026-07-03T09:30:00Z");
    arcs::verification::VerificationContext context{};
    const auto policy = make_policy();
    context.policy = &policy;
    context.store = &store;
    context.time_source = &time_source;

    const auto check = arcs::verification::ApprovalVerifier{}.check(action, context);
    EXPECT_EQ(check.status, arcs::verification::CheckStatus::Fail);
    EXPECT_EQ(check.detail, "approval store head no longer matches action candidate head");
}

} // namespace

#include <gtest/gtest.h>

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "core/resume/resume_service.hpp"
#include "store/store_memory.hpp"

namespace {

arcs::artifact::ArtifactVersion make_artifact(
    const std::string& artifact_id,
    const std::string& version_id,
    const std::string& type,
    const std::string& stream_key,
    nlohmann::json payload = nlohmann::json::object())
{
    arcs::artifact::ArtifactVersion artifact;
    artifact.artifact_id = artifact_id;
    artifact.version_id = version_id;
    artifact.type = type;
    if (type == "option") {
        artifact.schema_id = "arcs.option.v1";
    } else if (type == "policy") {
        artifact.schema_id = "arcs.policy.v1";
    } else if (type == "action_candidate") {
        artifact.schema_id = "arcs.action_candidate.report_emit.v1";
    } else if (type == "verification_report") {
        artifact.schema_id = "arcs.verification_report.v1";
    } else if (type == "approval_request") {
        artifact.schema_id = "arcs.approval_request.v1";
    } else {
        artifact.schema_id = "arcs.artifact_base.v1";
    }
    artifact.created_at = "2026-07-05T12:00:00Z";
    artifact.created_by = {.actor_type = "system", .id = "test"};
    artifact.source = {.kind = "internal", .ref = "unit"};
    artifact.trust = {.level = "high", .source_class = "system"};
    artifact.stream_key = stream_key;
    artifact.payload = std::move(payload);
    return artifact;
}

arcs::artifact::ArtifactVersion make_option()
{
    return make_artifact(
        "a_option",
        "v_option",
        "option",
        "task_id:t1",
        nlohmann::json{
            {"title", "Generate report"},
            {"human_summary", "Generate a JSON report"},
            {"steps", nlohmann::json::array({{{"kind", "emit_report"}, {"params", {{"format", "json"}}}}})},
            {"requires_permissions", nlohmann::json::array({"exec:report_emit"})},
            {"safety_level", "low"},
            {"policy_ref", {{"artifact_id", "a_policy"}, {"version_id", "v_policy"}}},
            {"request", "generate_report"},
        });
}

arcs::artifact::ArtifactVersion make_policy()
{
    return make_artifact(
        "a_policy",
        "v_policy",
        "policy",
        "task_id:t1",
        nlohmann::json{
            {"capabilities", nlohmann::json::array({"exec:report_emit"})},
            {"constraints", nlohmann::json::object()},
            {"verifier_rules", {{"hard_checks", nlohmann::json::array()}, {"soft_checks", nlohmann::json::array()}}},
            {"approval_required_for", nlohmann::json::array({"report_emit"})},
        });
}

arcs::artifact::ArtifactVersion make_candidate()
{
    return make_artifact(
        "a_candidate",
        "v_candidate",
        "action_candidate",
        "task_id:t1",
        nlohmann::json{
            {"action_id", "report_1"},
            {"type", "report_emit"},
            {"option_ref", {{"artifact_id", "a_option"}, {"version_id", "v_option"}}},
            {"policy_ref", {{"artifact_id", "a_policy"}, {"version_id", "v_policy"}}},
            {"params", {{"format", "json"}}},
            {"required_permissions", nlohmann::json::array({"exec:report_emit"})},
            {"safety_level", "low"},
            {"idempotency_key", "idem-report-1"},
        });
}

arcs::artifact::ArtifactVersion make_verification_report(arcs::verification::CheckStatus status, const std::string& version_id = "v_report")
{
    return make_artifact(
        "a_report",
        version_id,
        "verification_report",
        "task_id:t1",
        nlohmann::json{
            {"target", {{"artifact_id", "a_option"}, {"version_id", "v_option"}}},
            {"status", arcs::verification::to_string(status)},
            {"checks", nlohmann::json::array()},
            {"blockers", nlohmann::json::array()},
            {"recommendations", nlohmann::json::array()},
        });
}

arcs::artifact::ArtifactVersion make_request(
    const std::string& verification_version_id = "v_report",
    const std::string& risk_summary = "safety_level=low; action_type=report_emit")
{
    return make_artifact(
        "a_request",
        "v_request",
        "approval_request",
        "task_id:t1",
        nlohmann::json{
            {"target_option", {{"artifact_id", "a_option"}, {"version_id", "v_option"}}},
            {"policy_ref", {{"artifact_id", "a_policy"}, {"version_id", "v_policy"}}},
            {"verification_ref", {{"artifact_id", "a_report"}, {"version_id", verification_version_id}}},
            {"action_candidate_ref", {{"artifact_id", "a_candidate"}, {"version_id", "v_candidate"}}},
            {"requested_scope", "task_id:t1"},
            {"requested_at", "2026-07-05T12:00:00Z"},
            {"store_head_at_request", "v_report"},
            {"risk_summary", risk_summary},
        });
}

arcs::approval::ApprovalArtifact make_approval(
    const std::string& verification_version_id = "v_report",
    const std::string& risk_summary = "safety_level=low; action_type=report_emit")
{
    arcs::approval::ApprovalPayload payload{};
    payload.target_option = {"a_option", "v_option"};
    payload.policy_ref = {"a_policy", "v_policy"};
    payload.verification_ref = {"a_report", verification_version_id};
    payload.request_ref = {"a_request", "v_request"};
    payload.action_candidate_ref = {"a_candidate", "v_candidate"};
    payload.decision = arcs::approval::ApprovalDecision::Approve;
    payload.reason = "approved";
    payload.actor = {"human", "user:test"};
    payload.timestamp = "2026-07-05T12:00:00Z";
    payload.expires_at = "2099-07-05T13:00:00Z";
    payload.approval_scope = "task_id:t1";
    payload.store_head_at_approval = "v_candidate";
    payload.risk_summary = risk_summary;

    auto approval = arcs::approval::ApprovalGate{}.submit(payload);
    approval.stream_key = "task_id:t1";
    return approval;
}

void seed_valid_resume_store(arcs::store::StoreMemory& store)
{
    store.append_artifact(make_option());
    store.append_artifact(make_policy());
    store.append_artifact(make_candidate());
    store.append_artifact(make_verification_report(arcs::verification::CheckStatus::Pass));
    store.append_artifact(make_request());
}

} // namespace

TEST(ResumeServiceTest, LoadsPersistedVerificationReportInsteadOfSynthesizingOne)
{
    arcs::store::StoreMemory store;
    seed_valid_resume_store(store);

    const auto result = arcs::core::resume::ResumeService{}.resume_from_approval(make_approval(), store);

    ASSERT_TRUE(result.ok);
    EXPECT_EQ(result.option_report.status, arcs::verification::CheckStatus::Pass);
    EXPECT_EQ(result.option_report.target.artifact_id, "a_option");
    EXPECT_EQ(result.verification_report_artifact.artifact_id, "a_report");
}

TEST(ResumeServiceTest, RejectsNonPassingVerificationReport)
{
    arcs::store::StoreMemory store;
    store.append_artifact(make_option());
    store.append_artifact(make_policy());
    store.append_artifact(make_candidate());
    store.append_artifact(make_verification_report(arcs::verification::CheckStatus::Fail, "v_report_fail"));
    store.append_artifact(make_request("v_report_fail"));

    auto approval = make_approval("v_report_fail");

    const auto result = arcs::core::resume::ResumeService{}.resume_from_approval(approval, store);

    EXPECT_FALSE(result.ok);
    EXPECT_EQ(result.error_code, "resume.verification_not_pass");
}

TEST(ResumeServiceTest, RejectsRiskSummaryMismatchBetweenApprovalAndRequest)
{
    arcs::store::StoreMemory store;
    seed_valid_resume_store(store);

    auto approval = make_approval();
    approval.payload["risk_summary"] = "safety_level=high; action_type=report_emit";

    const auto result = arcs::core::resume::ResumeService{}.resume_from_approval(approval, store);

    EXPECT_FALSE(result.ok);
    EXPECT_EQ(result.error_code, "resume.risk_summary_mismatch");
}

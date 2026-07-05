/**
 * @file approval_service.cpp
 * @brief Implements ApprovalService, the pipeline stage responsible for the human
 *        (or demo) approval gate: building approval_request artifacts that reference
 *        the option/policy/verification/action-candidate chain, submitting a demo
 *        approval decision through the ApprovalGate, and promoting an approved action
 *        candidate into an executable action artifact.
 */

#include "core/services/approval_service.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>

#include "artifact/factory.hpp"

namespace arcs::core::services {
namespace {

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 string (YYYY-MM-DDTHH:MM:SSZ).
 * @return The formatted current UTC timestamp.
 */
std::string utc_now()
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

/**
 * @brief Constructs a base artifact version stamped with the current UTC time,
 *        forwarding all fields to arcs::artifact::factory::make_base_artifact.
 * @param type The artifact type.
 * @param schema_id The schema identifier the artifact payload conforms to.
 * @param stream_key The stream key the artifact belongs to.
 * @param actor_type The type of actor creating the artifact.
 * @param actor_id The identifier of the actor creating the artifact.
 * @param source_kind The kind of source that produced the artifact.
 * @param source_ref A reference describing the artifact's source.
 * @param trust_level The trust level assigned to the artifact.
 * @param trust_source_class The trust classification of the source.
 * @return The newly constructed base artifact version.
 */
arcs::artifact::ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class)
{
    return arcs::artifact::factory::make_base_artifact(
        type,
        schema_id,
        stream_key,
        actor_type,
        actor_id,
        source_kind,
        source_ref,
        trust_level,
        trust_source_class,
        utc_now());
}

} // namespace

/**
 * @brief Builds an "approval_request" artifact linking the option, policy,
 *        verification report, and action candidate that require human approval.
 * @param option The decision option artifact requiring approval.
 * @param policy The policy artifact governing the option.
 * @param verification_report The verification report artifact for the option.
 * @param action_candidate The action candidate artifact awaiting approval.
 * @param requested_at Timestamp at which approval was requested.
 * @param store_head_at_request Store head reference at the time of the request.
 * @param risk_summary Human-readable risk summary to attach to the request.
 * @return The constructed approval_request artifact version, with provenance
 *         linking it to all referenced parent artifacts.
 */
arcs::artifact::ArtifactVersion ApprovalService::create_approval_request(
    const arcs::artifact::ArtifactVersion& option,
    const arcs::artifact::ArtifactVersion& policy,
    const arcs::artifact::ArtifactVersion& verification_report,
    const arcs::artifact::ArtifactVersion& action_candidate,
    const std::string& requested_at,
    const std::string& store_head_at_request,
    const std::string& risk_summary) const
{
    auto artifact = make_artifact(
        "approval_request",
        "arcs.approval_request.v1",
        option.stream_key,
        "system",
        "kernel",
        "internal",
        "approval_request",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"target_option", {{"artifact_id", option.artifact_id}, {"version_id", option.version_id}}},
        {"policy_ref", {{"artifact_id", policy.artifact_id}, {"version_id", policy.version_id}}},
        {"verification_ref", {{"artifact_id", verification_report.artifact_id}, {"version_id", verification_report.version_id}}},
        {"action_candidate_ref", {{"artifact_id", action_candidate.artifact_id}, {"version_id", action_candidate.version_id}}},
        {"requested_scope", option.stream_key},
        {"requested_at", requested_at},
        {"store_head_at_request", store_head_at_request},
        {"risk_summary", risk_summary},
    };
    artifact.provenance.parents = {option.artifact_id, policy.artifact_id, verification_report.artifact_id, action_candidate.artifact_id};
    artifact.provenance.rules_applied = {"approval_request_gate"};
    artifact.provenance.transform = "request_approval";
    return artifact;
}

/**
 * @brief Submits a hard-coded "demo" approval (always Approve) through the
 *        ApprovalGate for the given option/policy/verification/request/action chain.
 * @param option The decision option artifact being approved.
 * @param policy The policy artifact governing the option.
 * @param verification_report The verification report artifact for the option.
 * @param approval_request The approval_request artifact this approval responds to.
 * @param action_candidate The action candidate artifact being approved.
 * @param approval_timestamp Timestamp at which the approval decision was made.
 * @param expires_at Timestamp at which the approval expires.
 * @param store_head_at_approval Store head reference at the time of approval.
 * @param risk_summary Human-readable risk summary to attach to the approval.
 * @return The resulting ApprovalArtifact, stamped with the option's stream key.
 */
arcs::approval::ApprovalArtifact ApprovalService::submit_demo_approval(
    const arcs::artifact::ArtifactVersion& option,
    const arcs::artifact::ArtifactVersion& policy,
    const arcs::artifact::ArtifactVersion& verification_report,
    const arcs::artifact::ArtifactVersion& approval_request,
    const arcs::artifact::ArtifactVersion& action_candidate,
    const std::string& approval_timestamp,
    const std::string& expires_at,
    const std::string& store_head_at_approval,
    const std::string& risk_summary) const
{
    arcs::approval::ApprovalGate approval_gate;
    arcs::approval::ApprovalPayload approval_payload{
        .target_option = {option.artifact_id, option.version_id},
        .policy_ref = {policy.artifact_id, policy.version_id},
        .verification_ref = {verification_report.artifact_id, verification_report.version_id},
        .request_ref = {approval_request.artifact_id, approval_request.version_id},
        .action_candidate_ref = {action_candidate.artifact_id, action_candidate.version_id},
        .decision = arcs::approval::ApprovalDecision::Approve,
        .reason = "demo approval for report_emit",
        .actor = {"human", "demo:cli"},
        .timestamp = approval_timestamp,
        .expires_at = expires_at,
        .approval_scope = option.stream_key,
        .store_head_at_approval = store_head_at_approval,
        .risk_summary = risk_summary,
    };

    auto approval_artifact = approval_gate.submit(approval_payload);
    approval_artifact.stream_key = option.stream_key;
    return approval_artifact;
}

/**
 * @brief Promotes an approved action candidate into an executable "action"
 *        artifact, rewriting its identifiers and attaching approval/verification
 *        references and provenance.
 * @param action_candidate The action candidate artifact to promote.
 * @param approval The approval artifact authorizing the promotion.
 * @return The promoted action artifact version, with type "action" and a new
 *         artifact_id/version_id derived from the candidate.
 */
arcs::artifact::ArtifactVersion ApprovalService::promote_action_candidate(
    const arcs::artifact::ArtifactVersion& action_candidate,
    const arcs::approval::ApprovalArtifact& approval) const
{
    auto action = action_candidate;
    const auto action_id = action_candidate.payload.value("action_id", std::string{});
    action.type = "action";
    action.schema_id = "arcs.action.report_emit.v1";
    action.artifact_id = action_id.empty() ? action_candidate.artifact_id + "_approved"
                                           : "a_exec_" + action_id;
    action.version_id = action_candidate.version_id + "_approved";
    action.payload["action_candidate_ref"] = {
        {"artifact_id", action_candidate.artifact_id},
        {"version_id", action_candidate.version_id},
    };
    action.payload["approval_ref"] = {
        {"artifact_id", approval.artifact_id},
        {"version_id", approval.version_id},
    };
    if (approval.payload.is_object() && approval.payload.contains("verification_ref")) {
        action.payload["verification_ref"] = approval.payload.at("verification_ref");
    }
    action.provenance.parents = {action_candidate.artifact_id, approval.artifact_id};
    action.provenance.rules_applied = {"approval_promote_action_candidate"};
    action.provenance.transform = "promote_action_candidate_to_action";
    return action;
}

} // namespace arcs::core::services

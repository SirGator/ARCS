/**
 * @file approval_gate.cpp
 * @brief Implements ApprovalGate::submit, which turns an ApprovalPayload
 *        into a signed "approval" artifact carrying links to the artifacts
 *        it approves/rejects/modifies/revokes.
 */

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "artifact/factory.hpp"
#include "artifact/ids.hpp"

namespace arcs::approval {

namespace {

/**
 * @brief Converts an ApprovalDecision enum value to its lowercase string
 *        representation used in the artifact payload.
 * @param decision The decision to convert.
 * @return The lowercase decision name, or "unknown" for an unrecognized value.
 */
std::string to_string(ApprovalDecision decision)
{
    switch (decision) {
        case ApprovalDecision::Approve:
            return "approve";
        case ApprovalDecision::Reject:
            return "reject";
        case ApprovalDecision::Modify:
            return "modify";
        case ApprovalDecision::Revoke:
            return "revoke";
    }

    return "unknown";
}

} // namespace

/**
 * @brief Builds an "approval" artifact from the given decision payload,
 *        recording the decision, actor, timing, and links to every
 *        referenced artifact in the payload and provenance.
 * @param in The approval payload describing the decision to record.
 * @return The resulting approval artifact.
 */
ApprovalArtifact ApprovalGate::submit(const ApprovalPayload& in)
{
    ApprovalArtifact a = arcs::artifact::factory::make_base_artifact(
        "approval",
        "arcs.approval.v1",
        in.target_option.artifact_id,
        in.actor.actor_type,
        in.actor.id,
        "internal",
        "approval_gate",
        "high",
        "human",
        in.timestamp);
    a.payload = nlohmann::json{
        {"target_option", {
            {"artifact_id", in.target_option.artifact_id},
            {"version_id", in.target_option.version_id}
        }},
        {"policy_ref", {
            {"artifact_id", in.policy_ref.artifact_id},
            {"version_id", in.policy_ref.version_id}
        }},
        {"verification_ref", {
            {"artifact_id", in.verification_ref.artifact_id},
            {"version_id", in.verification_ref.version_id}
        }},
        {"request_ref", {
            {"artifact_id", in.request_ref.artifact_id},
            {"version_id", in.request_ref.version_id}
        }},
        {"action_candidate_ref", {
            {"artifact_id", in.action_candidate_ref.artifact_id},
            {"version_id", in.action_candidate_ref.version_id}
        }},
        {"decision", to_string(in.decision)},
        {"reason", in.reason},
        {"actor", {
            {"actor_type", in.actor.actor_type},
            {"id", in.actor.id}
        }},
        {"timestamp", in.timestamp},
        {"expires_at", in.expires_at},
        {"approval_scope", in.approval_scope},
        {"store_head_at_approval", in.store_head_at_approval},
        {"risk_summary", in.risk_summary}
    };
    a.provenance.parents = {
        in.target_option.artifact_id,
        in.policy_ref.artifact_id,
        in.verification_ref.artifact_id,
        in.request_ref.artifact_id,
        in.action_candidate_ref.artifact_id,
    };
    a.provenance.rules_applied = {"approval_gate"};
    a.provenance.transform = "submit_approval";

    return a;
}

} // namespace arcs::approval

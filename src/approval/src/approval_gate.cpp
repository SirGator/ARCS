#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "artifact/factory.hpp"
#include "artifact/ids.hpp"

namespace arcs::approval {

namespace {

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
        {"decision", to_string(in.decision)},
        {"reason", in.reason},
        {"actor", {
            {"actor_type", in.actor.actor_type},
            {"id", in.actor.id}
        }},
        {"timestamp", in.timestamp},
        {"expires_at", in.expires_at}
    };
    a.provenance.parents = {in.target_option.artifact_id, in.policy_ref.artifact_id};
    a.provenance.rules_applied = {"approval_gate"};
    a.provenance.transform = "submit_approval";

    return a;
}

} // namespace arcs::approval

#include "approval.hpp"
#include "artifact/artifact.hpp"
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
    ApprovalArtifact a{};
    a.artifact_id = arcs::artifact::ids::new_artifact_id();
    a.version_id = arcs::artifact::ids::new_version_id();
    a.version = 1;
    a.type = "approval";
    a.schema_id = "arcs.approval.v1";
    a.schema_version = 1;

    a.created_at = in.timestamp;
    a.created_by = {
        .actor_type = in.actor.actor_type,
        .id = in.actor.id,
    };
    a.stream_key = in.target_option.artifact_id;
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

    return a;
}

} // namespace arcs::approval

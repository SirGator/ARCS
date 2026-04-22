#pragma once

#include <string>
#include <optional>

#include "artifact/artifact.hpp"

namespace arcs::approval {

struct ArtifactRef {
    std::string artifact_id;
    std::string version_id;
};

struct ActorRef {
    std::string actor_type; // human|system|model|executor
    std::string id;
};

enum class ApprovalDecision {
    Approve,
    Reject,
    Modify,
    Revoke
};

struct ApprovalPayload {
    ArtifactRef target_option;
    ArtifactRef policy_ref;
    ApprovalDecision decision;
    std::string reason;
    ActorRef actor;
    std::string timestamp;   // UTC ISO-8601
    std::string expires_at;  // UTC ISO-8601
};

using ApprovalArtifact = arcs::artifact::ArtifactVersion;

class IApprovalGate {
public:
    virtual ~IApprovalGate() = default;
    virtual ApprovalArtifact submit(const ApprovalPayload& decision) = 0;
};

class ApprovalGate final : public IApprovalGate {
public:
    ApprovalArtifact submit(const ApprovalPayload& decision) override;
};

} // namespace arcs::approval

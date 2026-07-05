/**
 * @file approval.hpp
 * @brief Defines the data model and gate interface for recording human (or
 *        system) approval decisions on proposed actions as ARCS artifacts.
 */

#pragma once

#include <string>
#include <optional>

#include "artifact/artifact.hpp"

namespace arcs::approval {

/**
 * @brief Reference to a specific version of an artifact, used to link an
 *        approval decision to the artifacts it pertains to.
 */
struct ArtifactRef {
    std::string artifact_id;
    std::string version_id;
};

/**
 * @brief Identifies who or what made an approval decision.
 */
struct ActorRef {
    std::string actor_type; // human|system|model|executor
    std::string id;
};

/**
 * @brief The possible outcomes of an approval review.
 */
enum class ApprovalDecision {
    Approve,
    Reject,
    Modify,
    Revoke
};

/**
 * @brief Input describing an approval decision to be recorded, including
 *        the artifacts it references, who made the decision, and its
 *        scope and validity window.
 */
struct ApprovalPayload {
    ArtifactRef target_option;
    ArtifactRef policy_ref;
    ArtifactRef verification_ref;
    ArtifactRef request_ref;
    ArtifactRef action_candidate_ref;
    ApprovalDecision decision;
    std::string reason;
    ActorRef actor;
    std::string timestamp;   // UTC ISO-8601
    std::string expires_at;  // UTC ISO-8601
    std::string approval_scope;
    std::string store_head_at_approval;
    std::string risk_summary;
};

using ApprovalArtifact = arcs::artifact::ArtifactVersion;

/**
 * @brief Interface for gates that record an approval decision as an
 *        artifact.
 */
class IApprovalGate {
public:
    virtual ~IApprovalGate() = default;

    /**
     * @brief Records an approval decision as a new artifact version.
     * @param decision The approval payload describing the decision.
     * @return The resulting approval artifact.
     */
    virtual ApprovalArtifact submit(const ApprovalPayload& decision) = 0;
};

/**
 * @brief Default approval gate implementation that builds an approval
 *        artifact from a decision payload via the artifact factory.
 */
class ApprovalGate final : public IApprovalGate {
public:
    ApprovalArtifact submit(const ApprovalPayload& decision) override;
};

} // namespace arcs::approval

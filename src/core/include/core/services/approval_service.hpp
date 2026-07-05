/**
 * @file approval_service.hpp
 * @brief Creates approval requests for actions requiring human sign-off,
 *        simulates approving them in demo mode, and promotes action
 *        candidates once approved.
 */

#pragma once

#include <string>

#include "approval.hpp"
#include "artifact/artifact.hpp"

namespace arcs::core::services {

/**
 * @brief Manages the approval lifecycle for an action candidate: raising a
 *        request, recording a (demo) approval decision, and promoting the
 *        candidate once approved.
 */
class ApprovalService {
public:
    /**
     * @brief Creates an approval request artifact for the given option and
     *        action candidate.
     * @param option Option under consideration.
     * @param policy Policy in effect at request time.
     * @param verification_report Verification report backing the request.
     * @param action_candidate Candidate action requiring approval.
     * @param requested_at Timestamp the request was raised.
     * @param store_head_at_request Store head reference at request time.
     * @param risk_summary Human-readable risk summary to attach.
     * @return The created approval request artifact.
     */
    arcs::artifact::ArtifactVersion create_approval_request(
        const arcs::artifact::ArtifactVersion& option,
        const arcs::artifact::ArtifactVersion& policy,
        const arcs::artifact::ArtifactVersion& verification_report,
        const arcs::artifact::ArtifactVersion& action_candidate,
        const std::string& requested_at,
        const std::string& store_head_at_request,
        const std::string& risk_summary) const;

    /**
     * @brief Simulates submitting an approval decision for a request, for
     *        demo-mode flows that bypass a real human approver.
     * @param option Option under consideration.
     * @param policy Policy in effect at approval time.
     * @param verification_report Verification report backing the approval.
     * @param approval_request Approval request being answered.
     * @param action_candidate Candidate action being approved.
     * @param approval_timestamp Timestamp the approval was granted.
     * @param expires_at Timestamp the approval expires.
     * @param store_head_at_approval Store head reference at approval time.
     * @param risk_summary Human-readable risk summary to attach.
     * @return The resulting approval artifact.
     */
    arcs::approval::ApprovalArtifact submit_demo_approval(
        const arcs::artifact::ArtifactVersion& option,
        const arcs::artifact::ArtifactVersion& policy,
        const arcs::artifact::ArtifactVersion& verification_report,
        const arcs::artifact::ArtifactVersion& approval_request,
        const arcs::artifact::ArtifactVersion& action_candidate,
        const std::string& approval_timestamp,
        const std::string& expires_at,
        const std::string& store_head_at_approval,
        const std::string& risk_summary) const;

    /**
     * @brief Promotes an action candidate to its final form once it carries
     *        a valid approval.
     * @param action_candidate Candidate action to promote.
     * @param approval Approval artifact authorizing the promotion.
     * @return The promoted action artifact.
     */
    arcs::artifact::ArtifactVersion promote_action_candidate(
        const arcs::artifact::ArtifactVersion& action_candidate,
        const arcs::approval::ApprovalArtifact& approval) const;
};

} // namespace arcs::core::services

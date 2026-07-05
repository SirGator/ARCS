/**
 * @file resume_service.hpp
 * @brief Reconstructs in-flight flow state from a persisted approval
 *        artifact, so a paused flow can be resumed after approval.
 */

#pragma once

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "store/store.hpp"
#include "verification/verifier.hpp"

namespace arcs::core::resume {

/**
 * @brief Result of reconstructing flow state from an approval artifact:
 *        the related option, policy, action candidate, and verification
 *        report, or an error if reconstruction failed.
 */
struct ApprovalResumeState {
    bool ok{false};
    std::string error_code;
    std::string error;
    arcs::artifact::ArtifactVersion option;
    arcs::artifact::ArtifactVersion policy;
    arcs::artifact::ArtifactVersion action_candidate;
    arcs::artifact::ArtifactVersion approval_request;
    arcs::artifact::ArtifactVersion verification_report_artifact;
    arcs::verification::VerificationReportData option_report;
};

/**
 * @brief Rebuilds the state needed to resume a flow that was paused for
 *        approval, by looking up the related artifacts in the store.
 */
class ResumeService {
public:
    /**
     * @brief Resolves the option, policy, action candidate, and report tied
     *        to an approval artifact so the flow can continue.
     * @param approval Approval artifact the flow was paused on.
     * @param store Store to resolve the related artifacts from.
     * @return The reconstructed resume state, or a failed state on error.
     */
    ApprovalResumeState resume_from_approval(
        const arcs::approval::ApprovalArtifact& approval,
        const arcs::store::IStore& store) const;
};

} // namespace arcs::core::resume

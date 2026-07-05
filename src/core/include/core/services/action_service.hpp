/**
 * @file action_service.hpp
 * @brief Turns a decided option into a concrete, executable action
 *        candidate, promotes it once approved, and builds the verification
 *        context used to check it.
 */

#pragma once

#include <optional>

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "store/store.hpp"
#include "verification/verifier.hpp"

namespace arcs::core::services {

class VerificationService;
class ApprovalService;

/**
 * @brief Materializes action candidates from options, summarizes their
 *        risk, promotes approved candidates into final actions, and builds
 *        the verification context used to check them against policy.
 */
class ActionService {
public:
    /**
     * @brief Materializes an executable action candidate for the given
     *        option under the given policy, if one can be produced.
     * @param option Option artifact to materialize an action for.
     * @param policy Policy artifact constraining the action.
     * @return The materialized action candidate, or empty if none applies.
     */
    std::optional<arcs::artifact::ArtifactVersion> materialize_candidate(
        const arcs::artifact::ArtifactVersion& option,
        const arcs::artifact::ArtifactVersion& policy) const;

    /**
     * @brief Produces a human-readable risk summary for an action candidate.
     * @param option Option the action candidate was derived from.
     * @param action_candidate Action candidate to summarize.
     * @return Textual risk summary.
     */
    std::string risk_summary(
        const arcs::artifact::ArtifactVersion& option,
        const arcs::artifact::ArtifactVersion& action_candidate) const;

    /**
     * @brief Promotes an action candidate to a final action once it has
     *        been approved.
     * @param action_candidate Candidate action to promote.
     * @param approval Approval artifact authorizing the promotion.
     * @param approval_service Service used to validate/apply the approval.
     * @return The promoted action artifact.
     */
    arcs::artifact::ArtifactVersion promote_candidate(
        const arcs::artifact::ArtifactVersion& action_candidate,
        const arcs::approval::ApprovalArtifact& approval,
        const ApprovalService& approval_service) const;

    /**
     * @brief Builds the verification context (policy, permissions, schemas,
     *        store, time source) used to verify an action.
     * @param policy Policy artifact in effect.
     * @param permissions Effective permissions to verify against.
     * @param schemas Schema registry used to validate artifacts.
     * @param store Store used to resolve related artifacts.
     * @param time_source Time source used for time-dependent checks.
     * @return The assembled verification context.
     */
    arcs::verification::VerificationContext build_verification_context(
        const arcs::artifact::ArtifactVersion& policy,
        const arcs::reducer::EffectivePermissions& permissions,
        const arcs::schema::SchemaRegistry& schemas,
        const arcs::store::IStore& store,
        const arcs::reducer::ITimeSource& time_source) const;
};

} // namespace arcs::core::services

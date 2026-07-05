/**
 * @file decision_service.hpp
 * @brief Produces the final decision artifact that summarizes a flow's
 *        outcome for a given option.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "verification/verifier.hpp"

namespace arcs::core::services {

/**
 * @brief Builds the decision artifact recording the flow's final status,
 *        reason, and links to the related approval/action/execution
 *        artifacts.
 */
class DecisionService {
public:
    /**
     * @brief Creates a decision artifact summarizing the outcome for an
     *        option.
     * @param option Option the decision applies to.
     * @param report Verification report backing the decision.
     * @param status Final decision status (e.g. approved/blocked).
     * @param reason Human-readable reason for the decision.
     * @param approval_artifact_id Id of the related approval artifact, if any.
     * @param action_artifact_id Id of the related action artifact, if any.
     * @param execution_result_artifact_id Id of the related execution result
     *        artifact, if any.
     * @return The constructed decision artifact.
     */
    arcs::artifact::ArtifactVersion make_decision(
        const arcs::artifact::ArtifactVersion& option,
        const arcs::verification::VerificationReportData& report,
        const std::string& status,
        const std::string& reason,
        const std::string& approval_artifact_id,
        const std::string& action_artifact_id,
        const std::string& execution_result_artifact_id) const;
};

} // namespace arcs::core::services

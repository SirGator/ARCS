/**
 * @file verification_service.hpp
 * @brief Builds verification plans from policy, runs verification against
 *        actions, and wraps the results as report artifacts.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "schema/schema_registry.hpp"
#include "verification/verifier.hpp"

namespace arcs::core::services {

/**
 * @brief Derives verification plans from policy and evaluates actions
 *        against them, producing verification report artifacts.
 */
class VerificationService {
public:
    /**
     * @brief Lists the verifier rule names configured by a policy.
     * @param policy Policy artifact to inspect.
     * @return Names of the verifier rules the policy configures.
     */
    std::vector<std::string> verifier_rule_names(const arcs::artifact::ArtifactVersion& policy) const;

    /**
     * @brief Builds the verification plan derived from a policy.
     * @param policy Policy artifact to build a plan from.
     * @return The constructed verification plan.
     */
    arcs::verification::VerificationPlan build_policy_plan(
        const arcs::artifact::ArtifactVersion& policy) const;

    /**
     * @brief Records checks that could not be evaluated into a verification
     *        report.
     * @param report Report to append the unsupported checks to.
     * @param unsupported_checks Names of checks that could not be run.
     */
    void append_unsupported_checks(
        arcs::verification::VerificationReportData& report,
        const std::vector<std::string>& unsupported_checks) const;

    /**
     * @brief Verifies an action against the given verification context.
     * @param action Action artifact to verify.
     * @param context Verification context (policy, permissions, etc.) to
     *        verify against.
     * @return The resulting verification report data.
     */
    arcs::verification::VerificationReportData verify_action(
        const arcs::artifact::ArtifactVersion& action,
        const arcs::verification::VerificationContext& context) const;

    /**
     * @brief Wraps verification report data into a named report artifact.
     * @param target Artifact the report is about.
     * @param report Verification report data to wrap.
     * @param artifact_id Identifier to assign the report artifact.
     * @param version_id Version identifier to assign the report artifact.
     * @return The constructed report artifact.
     */
    arcs::artifact::ArtifactVersion make_named_report_artifact(
        const arcs::artifact::ArtifactVersion& target,
        const arcs::verification::VerificationReportData& report,
        const std::string& artifact_id,
        const std::string& version_id) const;
};

} // namespace arcs::core::services

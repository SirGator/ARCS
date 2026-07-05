/**
 * @file policy_head_verifier.cpp
 * @brief Implements PolicyHeadVerifier::check, which detects policy drift
 *        by comparing a target's embedded policy reference against the
 *        current policy head in the verification context.
 */
#include "verification/verifier.hpp"

#include <string>

namespace arcs::verification {

/**
 * @brief Checks that the target's payload.policy_ref matches the current
 *        policy head supplied in the context, failing if it points at a
 *        stale policy version.
 * @param target Artifact version being verified; expected to carry a
 *               "policy_ref" object in its payload.
 * @param context Verification context; must provide the current policy.
 * @return VerificationCheck named "policy_drift" with the outcome.
 */
VerificationCheck PolicyHeadVerifier::check(
    const ArtifactVersion& target,
    const VerificationContext& context) const
{
    VerificationCheck result{};
    result.name = "policy_drift";

    if (context.policy == nullptr) {
        result.status = CheckStatus::Unknown;
        result.detail = "policy missing in verification context";
        return result;
    }

    if (!target.payload.is_object() || !target.payload.contains("policy_ref") ||
        !target.payload.at("policy_ref").is_object()) {
        result.status = CheckStatus::Unknown;
        result.detail = "target.policy_ref missing";
        return result;
    }

    const auto& policy_ref = target.payload.at("policy_ref");
    const auto artifact_id = policy_ref.value("artifact_id", std::string{});
    const auto version_id = policy_ref.value("version_id", std::string{});

    if (artifact_id.empty() || version_id.empty()) {
        result.status = CheckStatus::Unknown;
        result.detail = "target.policy_ref malformed";
        return result;
    }

    if (artifact_id != context.policy->artifact_id || version_id != context.policy->version_id) {
        result.status = CheckStatus::Fail;
        result.detail = "option.policy_ref does not match current policy head";
        return result;
    }

    result.status = CheckStatus::Pass;
    result.detail = "policy head matches option binding";
    return result;
}

} // namespace arcs::verification

/**
 * @file authority_verifier.cpp
 * @brief Implements AuthorityVerifier: determines whether a target
 *        requires an elevated authority capability (policy edit or
 *        permission grant) and, if so, checks that the acting principal
 *        holds it.
 */
#include "verification/authority_verifier.hpp"

#include <algorithm>
#include <string>

namespace arcs::verification {

bool AuthorityVerifier::has_capability(
    const arcs::reducer::EffectivePermissions& permissions,
    const std::string& capability
) const {
    return std::find(
        permissions.capabilities.begin(),
        permissions.capabilities.end(),
        capability
    ) != permissions.capabilities.end();
}

/**
 * @brief Determines whether the target requires the "policy:edit" or
 *        "perm:grant" capability (based on its type, or its action type
 *        when the target is an "action"), and if so, checks that the
 *        context's effective permissions include it.
 * @param target Artifact version being verified.
 * @param ctx Verification context, including the effective permissions
 *            of the acting principal.
 * @return VerificationCheck named "authority" with the outcome.
 */
VerificationCheck AuthorityVerifier::check(
    const arcs::artifact::ArtifactVersion& target,
    const VerificationContext& ctx) const
{
    VerificationCheck result{};
    result.name = "authority";

    bool needs_policy_edit = false;
    bool needs_perm_grant = false;

    if (target.type == "policy") {
        needs_policy_edit = true;
    }

    if (target.type == "permission_grant") {
        needs_perm_grant = true;
    }

    if (target.type == "action" && target.payload.contains("type")) {
        const auto action_type = target.payload.at("type").get<std::string>();

        if (action_type == "policy_update") {
            needs_policy_edit = true;
        }

        if (action_type == "permission_grant") {
            needs_perm_grant = true;
        }
    }

    if (!needs_policy_edit && !needs_perm_grant) {
        result.status = CheckStatus::Pass;
        result.detail = "target does not require authority capability";
        return result;
    }

    if (needs_policy_edit && !has_capability(ctx.permissions, "policy:edit")) {
        result.status = CheckStatus::Fail;
        result.detail = "missing capability: policy:edit";
        return result;
    }

    if (needs_perm_grant && !has_capability(ctx.permissions, "perm:grant")) {
        result.status = CheckStatus::Fail;
        result.detail = "missing capability: perm:grant";
        return result;
    }

    result.status = CheckStatus::Pass;
    result.detail = "required authority capability present";

    return result;
}

} // namespace arcs::verification

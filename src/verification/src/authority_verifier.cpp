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

VerificationReport AuthorityVerifier::check(
    const arcs::artifact::ArtifactVersion& target,
    const VerificationContext& ctx) const
{
    VerificationReport report{};
    report.target_artifact_id = target.artifact_id;
    report.target_version_id = target.version_id;
    report.verifier_name = "AuthorityVerifier";

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
        report.status = "pass";
        report.checks.push_back({
            "authority",
            "pass",
            "target does not require authority capability"
        });
        return report;
    }

    if (needs_policy_edit && !has_capability(ctx.permissions, "policy:edit")) {
        report.status = "fail";
        report.checks.push_back({
            "authority",
            "fail",
            "missing capability: policy:edit"
        });
        report.blockers.push_back("missing capability: policy:edit");
        return report;
    }

    if (needs_perm_grant && !has_capability(ctx.permissions, "perm:grant")) {
        report.status = "fail";
        report.checks.push_back({
            "authority",
            "fail",
            "missing capability: perm:grant"
        });
        report.blockers.push_back("missing capability: perm:grant");
        return report;
    }

    report.status = "pass";
    report.checks.push_back({
        "authority",
        "pass",
        "required authority capability present"
    });

    return report;
}

} // namespace arcs::verification

/**
 * @file authority_verifier.hpp
 * @brief Declares AuthorityVerifier, which checks that the acting principal
 *        holds the elevated capabilities required for authority-sensitive
 *        targets (e.g. policy edits or permission grants) before they are
 *        allowed to proceed.
 */
#pragma once

#include <string>

#include "verification/verifier.hpp"
#include "reducer/permission_reducer.hpp"

namespace arcs::verification {

/**
 * @brief Verifier that enforces authority requirements for targets which
 *        modify policies or grant permissions.
 *
 * Determines whether the target under verification requires an elevated
 * capability (such as "policy:edit" or "perm:grant") and, if so, checks
 * that the effective permissions in the verification context grant it.
 * Targets that do not require an authority capability pass trivially.
 */
class AuthorityVerifier final : public IVerifier {
public:
    /**
     * @brief Verifies that the principal has the authority capability
     *        required to act on the given target, if any is required.
     * @param target Artifact version being verified.
     * @param ctx Verification context, including the effective permissions
     *            of the acting principal.
     * @return A VerificationCheck describing the pass/fail/unknown outcome
     *         of the authority check.
     */
    VerificationCheck check(
        const arcs::artifact::ArtifactVersion& target,
        const VerificationContext& ctx) const override;

private:
    /**
     * @brief Checks whether a given capability is present in a set of
     *        effective permissions.
     * @param permissions Effective permissions to search.
     * @param capability Capability identifier to look for (e.g. "policy:edit").
     * @return True if the capability is present, false otherwise.
     */
    bool has_capability(
        const arcs::reducer::EffectivePermissions& permissions,
        const std::string& capability
    ) const;
};

} // namespace arcs::verification

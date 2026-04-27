#pragma once

#include <string>

#include "verification/verifier.hpp"
#include "reducer/permission_reducer.hpp"

namespace arcs::verification {

class AuthorityVerifier final : public IVerifier {
public:
    VerificationCheck check(
        const arcs::artifact::ArtifactVersion& target,
        const VerificationContext& ctx) const override;

private:
    bool has_capability(
        const arcs::reducer::EffectivePermissions& permissions,
        const std::string& capability
    ) const;
};

} // namespace arcs::verification

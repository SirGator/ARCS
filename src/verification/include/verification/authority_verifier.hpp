#pragma once

#include <string>
#include <vector>

#include "verification/verifier.hpp"
#include "reducer/permission_reducer.hpp"

namespace arcs::verification {

struct VerificationReportEntry {
    std::string name;
    std::string status;
    std::string detail;
};

struct VerificationReport {
    std::string target_artifact_id;
    std::string target_version_id;
    std::string verifier_name;
    std::string status;
    std::vector<VerificationReportEntry> checks;
    std::vector<std::string> blockers;
};

class AuthorityVerifier final {
public:
    VerificationReport check(
        const arcs::artifact::ArtifactVersion& target,
        const VerificationContext& ctx) const;

private:
    bool has_capability(
        const arcs::reducer::EffectivePermissions& permissions,
        const std::string& capability
    ) const;
};

} // namespace arcs::verification

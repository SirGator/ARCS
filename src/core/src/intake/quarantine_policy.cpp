#include "core/intake/quarantine_policy.hpp"

namespace arcs::core::intake {

IntakeResult QuarantinePolicy::reject(
    const arcs::artifact::ArtifactVersion& artifact,
    const std::string& reason,
    const std::string& stage,
    arcs::ingress::QuarantineStore* quarantine) const
{
    if (quarantine != nullptr) {
        quarantine->store(arcs::ingress::QuarantinedEvent{
            .artifact = artifact,
            .rejection_reason = reason,
            .rejected_at = artifact.created_at,
            .rejection_stage = stage,
        });
    }

    return IntakeResult{
        .accepted = false,
        .artifact_ref = std::nullopt,
        .rejection_reason = reason,
        .diagnostics = {arcs::core::Diagnostic{
            .code = "intake.reject",
            .severity = arcs::core::DiagnosticSeverity::Error,
            .message = reason,
            .stage = stage,
            .artifact_id = artifact.artifact_id,
        }},
    };
}

} // namespace arcs::core::intake

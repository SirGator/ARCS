#include "core/intake/provenance_builder.hpp"

namespace arcs::core::intake {

void ProvenanceBuilder::apply(arcs::artifact::ArtifactVersion& artifact, const AdapterSubmission& submission) const
{
    artifact.provenance.rules_applied = {"adapter_submission_intake"};
    artifact.provenance.transform = submission.adapter_id.empty()
        ? "adapter_submission"
        : "adapter_submission:" + submission.adapter_id;

    if (submission.metadata.is_object()) {
        if (submission.metadata.contains("parent_artifact_id") && submission.metadata.at("parent_artifact_id").is_string()) {
            const auto parent = submission.metadata.at("parent_artifact_id").get<std::string>();
            if (!parent.empty()) {
                artifact.provenance.parents.push_back(parent);
            }
        }
    }
}

} // namespace arcs::core::intake

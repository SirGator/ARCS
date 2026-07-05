#pragma once

#include <string>

#include "core/commit/commit_service.hpp"
#include "core/intake/adapter_submission.hpp"
#include "core/intake/intake_result.hpp"
#include "core/intake/provenance_builder.hpp"
#include "core/intake/quarantine_policy.hpp"
#include "core/intake/schema_gate.hpp"
#include "ingress/quarantine.hpp"
#include "store/store.hpp"

namespace arcs::core::intake {

class ArtifactIntakeService {
public:
    ArtifactIntakeService(
        arcs::store::IStore& store,
        const arcs::schema::SchemaRegistry& schema_registry,
        const arcs::core::commit::CommitService& commit_service);

    IntakeResult accept(
        const AdapterSubmission& submission,
        arcs::ingress::QuarantineStore* quarantine = nullptr) const;

private:
    arcs::store::IStore& store_;
    SchemaGate schema_gate_;
    ProvenanceBuilder provenance_builder_;
    QuarantinePolicy quarantine_policy_;
    const arcs::core::commit::CommitService& commit_service_;
};

} // namespace arcs::core::intake

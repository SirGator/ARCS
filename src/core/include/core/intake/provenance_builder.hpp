#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "core/intake/adapter_submission.hpp"

namespace arcs::core::intake {

class ProvenanceBuilder {
public:
    void apply(arcs::artifact::ArtifactVersion& artifact, const AdapterSubmission& submission) const;
};

} // namespace arcs::core::intake

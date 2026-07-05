#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "core/intake/intake_result.hpp"
#include "ingress/quarantine.hpp"

namespace arcs::core::intake {

class QuarantinePolicy {
public:
    IntakeResult reject(
        const arcs::artifact::ArtifactVersion& artifact,
        const std::string& reason,
        const std::string& stage,
        arcs::ingress::QuarantineStore* quarantine) const;
};

} // namespace arcs::core::intake

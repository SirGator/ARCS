#pragma once

#include <optional>
#include <string>
#include <vector>

#include "core/flow_result.hpp"

namespace arcs::core::intake {

struct ArtifactRef {
    std::string artifact_id;
    std::string version_id;
};

struct IntakeResult {
    bool accepted{false};
    std::optional<ArtifactRef> artifact_ref;
    std::string rejection_reason;
    std::vector<arcs::core::Diagnostic> diagnostics;
};

} // namespace arcs::core::intake

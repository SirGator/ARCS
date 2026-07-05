#pragma once

#include <string>

#include "core/flow.hpp"
#include "core/runtime/core_request.hpp"
#include "core/runtime/core_runtime.hpp"
#include "core/services/run_artifacts_service.hpp"
#include "interpretation/config.hpp"

namespace arcs::app {

class RuntimeSession {
public:
    std::string run_text(
        const std::string& input,
        const arcs::interpretation::InterpretationApiConfig* interpretation_config,
        const arcs::core::FlowOptions& options);

private:
    arcs::core::runtime::CoreRuntime runtime_;
    arcs::core::services::RunArtifactsService output_service_;
};

} // namespace arcs::app

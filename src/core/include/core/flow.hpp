#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "interpretation/config.hpp"

namespace arcs::core {

struct FlowOptions {
    bool enable_demo_controls{false};
};

std::string run_text_flow(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config = nullptr,
    const FlowOptions& options = {});

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact);

std::string run_text_flow(
    const arcs::artifact::ArtifactVersion& input_artifact,
    const FlowOptions& options = {});

} // namespace arcs::core

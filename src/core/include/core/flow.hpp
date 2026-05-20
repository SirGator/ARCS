#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "interpretation/config.hpp"

namespace arcs::core {

std::string run_text_flow(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config = nullptr);

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact);

} // namespace arcs::core

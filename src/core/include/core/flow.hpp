#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::core {

std::string run_text_flow(const std::string& input);

std::string run_text_flow(const arcs::artifact::ArtifactVersion& input_artifact);

} // namespace arcs::core

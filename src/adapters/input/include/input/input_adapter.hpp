#pragma once

#include <istream>
#include <string>

#include "artifact/artifact.hpp"

namespace arcs::input {

class CliTextInputAdapter {
public:
    std::string read(std::istream& in) const;

    arcs::artifact::ArtifactVersion read_artifact(
        std::istream& in,
        const std::string& source_ref = "cli",
        const std::string& stream_key = "session:cli") const;

    arcs::artifact::ArtifactVersion normalize(
        const std::string& raw_text,
        const std::string& source_ref = "cli",
        const std::string& stream_key = "session:cli") const;
};

} // namespace arcs::input

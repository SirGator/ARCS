#include "input/input_adapter.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <string>
#include <sstream>

#include "artifact/factory.hpp"

namespace arcs::input {

std::string CliTextInputAdapter::read(std::istream& in) const
{
    std::string line;
    std::getline(in, line);
    return line;
}

arcs::artifact::ArtifactVersion CliTextInputAdapter::normalize(
    const std::string& raw_text,
    const std::string& source_ref,
    const std::string& stream_key) const
{
    auto artifact = arcs::artifact::factory::make_base_artifact(
        "input",
        "arcs.input.v1",
        stream_key,
        "human",
        "user:cli",
        "chat",
        source_ref,
        "high",
        "human");
    artifact.tags = {"input", "raw"};
    artifact.payload = {
        {"raw_text", raw_text},
        {"source", source_ref}
    };
    artifact.provenance.rules_applied = {"input_normalized"};
    artifact.provenance.transform = "normalize_input";

    return artifact;
}

arcs::artifact::ArtifactVersion CliTextInputAdapter::read_artifact(
    std::istream& in,
    const std::string& source_ref,
    const std::string& stream_key) const
{
    return normalize(read(in), source_ref, stream_key);
}

} // namespace arcs::input

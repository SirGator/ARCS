#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::artifact::factory {

ArtifactVersion make_base_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& created_by_actor_type,
    const std::string& created_by_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class,
    const std::string& created_at = {});

} // namespace arcs::artifact::factory

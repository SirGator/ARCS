#pragma once

#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "artifact/actor.hpp"
#include "artifact/source.hpp"
#include "artifact/trust.hpp"
#include "artifact/provenance.hpp"

namespace arcs {

struct ArtifactVersion {
    std::string artifact_id;
    std::string version_id;
    int version{1};

    std::string type;
    std::string schema_id;
    int schema_version{1};

    std::string created_at;

    ActorRef created_by;
    SourceRef source;
    TrustInfo trust;

    std::string stream_key;
    std::vector<std::string> tags;

    nlohmann::json payload;
    Provenance provenance;
};

} // namespace arcs
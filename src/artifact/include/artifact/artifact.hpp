/**
 * @file artifact.hpp
 * @brief Defines ArtifactVersion, the core data structure of ARCS: a single
 *        versioned, provenance-tracked unit of data flowing through the
 *        system (e.g. an ingress event, task, claim, or evidence record).
 */
#pragma once

#include <string>
#include <vector>
#include <nlohmann/json.hpp>

#include "artifact/actor.hpp"
#include "artifact/source.hpp"
#include "artifact/trust.hpp"
#include "artifact/provenance.hpp"

namespace arcs::artifact {

/**
 * @brief A single immutable version of an artifact: identity/versioning
 *        fields, schema binding, who/where/how it was created, its trust
 *        classification, an arbitrary JSON payload, and its provenance
 *        trail. This is the canonical unit that flows through ARCS.
 */
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

} // namespace arcs::artifact

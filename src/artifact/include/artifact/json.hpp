#pragma once
#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"

namespace arcs {

using nlohmann::json;

void to_json(json& j, const ActorRef& v);
void from_json(const json& j, ActorRef& v);

void to_json(json& j, const SourceRef& v);
void from_json(const json& j, SourceRef& v);

void to_json(json& j, const TrustInfo& v);
void from_json(const json& j, TrustInfo& v);

void to_json(json& j, const ModelUsage& v);
void from_json(const json& j, ModelUsage& v);

void to_json(json& j, const Provenance& v);
void from_json(const json& j, Provenance& v);

void to_json(json& j, const ArtifactVersion& v);
void from_json(const json& j, ArtifactVersion& v);

}

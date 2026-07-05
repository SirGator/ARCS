/**
 * @file json.hpp
 * @brief Declares nlohmann::json (de)serialization functions (to_json /
 *        from_json) for all artifact model types, enabling ArtifactVersion
 *        and its sub-structures to be converted to and from JSON.
 */
#pragma once
#include <nlohmann/json.hpp>

#include "artifact/artifact.hpp"

namespace arcs::artifact {

using nlohmann::json;

/// @brief Serializes an ActorRef to JSON.
void to_json(json& j, const ActorRef& v);
/// @brief Deserializes an ActorRef from JSON. Throws if actor_type is not
///        one of the allowed values.
void from_json(const json& j, ActorRef& v);

/// @brief Serializes a SourceRef to JSON.
void to_json(json& j, const SourceRef& v);
/// @brief Deserializes a SourceRef from JSON. Throws if kind is not one of
///        the allowed values.
void from_json(const json& j, SourceRef& v);

/// @brief Serializes a TrustInfo to JSON.
void to_json(json& j, const TrustInfo& v);
/// @brief Deserializes a TrustInfo from JSON. Throws if level or
///        source_class is not one of the allowed values.
void from_json(const json& j, TrustInfo& v);

/// @brief Serializes a ModelUsage to JSON.
void to_json(json& j, const ModelUsage& v);
/// @brief Deserializes a ModelUsage from JSON.
void from_json(const json& j, ModelUsage& v);

/// @brief Serializes a Provenance to JSON.
void to_json(json& j, const Provenance& v);
/// @brief Deserializes a Provenance from JSON. Missing optional fields
///        default to empty.
void from_json(const json& j, Provenance& v);

/// @brief Serializes an ArtifactVersion to JSON.
void to_json(json& j, const ArtifactVersion& v);
/// @brief Deserializes an ArtifactVersion from JSON. Missing optional
///        fields (tags, provenance) default to empty.
void from_json(const json& j, ArtifactVersion& v);

} // namespace arcs::artifact

/**
 * @file json.cpp
 * @brief Implements to_json/from_json conversions between the artifact
 *        model types and nlohmann::json, including validation of
 *        enum-like string fields on deserialization.
 */
#include "artifact/json.hpp"

#include <stdexcept>

namespace arcs::artifact {

namespace {

/**
 * @brief Checks that a value is one of an allowed set of strings, throwing
 *        if not. Used to validate enum-like fields when deserializing.
 * @param value The value to check.
 * @param allowed The set of allowed values.
 * @param field_name Name of the field, used in the error message.
 */
void require_value(
    const std::string& value,
    const std::initializer_list<const char*> allowed,
    const char* field_name)
{
    for (const char* candidate : allowed)
    {
        if (value == candidate)
        {
            return;
        }
    }

    throw std::invalid_argument(std::string("invalid value for ") + field_name + ": " + value);
}

} // namespace

/// @brief Serializes an ActorRef to JSON.
void to_json(json& j, const ActorRef& v)
{
    j = json{{"actor_type", v.actor_type}, {"id", v.id}};
}

/// @brief Deserializes an ActorRef from JSON, validating actor_type.
void from_json(const json& j, ActorRef& v)
{
    j.at("actor_type").get_to(v.actor_type);
    j.at("id").get_to(v.id);
    require_value(v.actor_type, {"human", "system", "model", "executor"}, "actor_type");
}

/// @brief Serializes a SourceRef to JSON.
void to_json(json& j, const SourceRef& v)
{
    j = json{{"kind", v.kind}, {"ref", v.ref}};
}

/// @brief Deserializes a SourceRef from JSON, validating kind.
void from_json(const json& j, SourceRef& v)
{
    j.at("kind").get_to(v.kind);
    j.at("ref").get_to(v.ref);
    require_value(v.kind, {"chat", "file", "api", "sensor", "timer", "internal"}, "source.kind");
}

/// @brief Serializes a TrustInfo to JSON.
void to_json(json& j, const TrustInfo& v)
{
    j = json{{"level", v.level}, {"source_class", v.source_class}};
}

/// @brief Deserializes a TrustInfo from JSON, validating level and
///        source_class.
void from_json(const json& j, TrustInfo& v)
{
    j.at("level").get_to(v.level);
    j.at("source_class").get_to(v.source_class);
    require_value(v.level, {"low", "medium", "high"}, "trust.level");
    require_value(v.source_class, {"human", "system", "model", "external"}, "trust.source_class");
}

/// @brief Serializes a ModelUsage to JSON.
void to_json(json& j, const ModelUsage& v)
{
    j = json{
        {"name", v.name},
        {"prompt_hash", v.prompt_hash},
        {"inputs", v.inputs},
        {"temperature", v.temperature},
        {"raw_output_hash", v.raw_output_hash}
    };
}

/// @brief Deserializes a ModelUsage from JSON.
void from_json(const json& j, ModelUsage& v)
{
    j.at("name").get_to(v.name);
    j.at("prompt_hash").get_to(v.prompt_hash);
    j.at("inputs").get_to(v.inputs);
    j.at("temperature").get_to(v.temperature);
    j.at("raw_output_hash").get_to(v.raw_output_hash);
}

/// @brief Serializes a Provenance to JSON.
void to_json(json& j, const Provenance& v)
{
    j = json{
        {"parents", v.parents},
        {"rules_applied", v.rules_applied},
        {"models_used", v.models_used},
        {"transform", v.transform}
    };
}

/// @brief Deserializes a Provenance from JSON. Each field defaults to
///        empty if absent from the JSON object.
void from_json(const json& j, Provenance& v)
{
    if (j.contains("parents"))
    {
        j.at("parents").get_to(v.parents);
    }
    else
    {
        v.parents.clear();
    }

    if (j.contains("rules_applied"))
    {
        j.at("rules_applied").get_to(v.rules_applied);
    }
    else
    {
        v.rules_applied.clear();
    }

    if (j.contains("models_used"))
    {
        j.at("models_used").get_to(v.models_used);
    }
    else
    {
        v.models_used.clear();
    }

    if (j.contains("transform"))
    {
        j.at("transform").get_to(v.transform);
    }
    else
    {
        v.transform.clear();
    }
}

/// @brief Serializes an ArtifactVersion to JSON.
void to_json(json& j, const ArtifactVersion& v)
{
    j = json{
        {"artifact_id", v.artifact_id},
        {"version_id", v.version_id},
        {"version", v.version},
        {"type", v.type},
        {"schema_id", v.schema_id},
        {"schema_version", v.schema_version},
        {"created_at", v.created_at},
        {"created_by", v.created_by},
        {"source", v.source},
        {"trust", v.trust},
        {"stream_key", v.stream_key},
        {"tags", v.tags},
        {"payload", v.payload},
        {"provenance", v.provenance}
    };
}

/// @brief Deserializes an ArtifactVersion from JSON. The tags and
///        provenance fields default to empty if absent.
void from_json(const json& j, ArtifactVersion& v)
{
    j.at("artifact_id").get_to(v.artifact_id);
    j.at("version_id").get_to(v.version_id);
    j.at("version").get_to(v.version);
    j.at("type").get_to(v.type);
    j.at("schema_id").get_to(v.schema_id);
    j.at("schema_version").get_to(v.schema_version);
    j.at("created_at").get_to(v.created_at);
    j.at("created_by").get_to(v.created_by);
    j.at("source").get_to(v.source);
    j.at("trust").get_to(v.trust);
    j.at("stream_key").get_to(v.stream_key);

    if (j.contains("tags"))
    {
        j.at("tags").get_to(v.tags);
    }
    else
    {
        v.tags.clear();
    }

    j.at("payload").get_to(v.payload);

    if (j.contains("provenance"))
    {
        j.at("provenance").get_to(v.provenance);
    }
    else
    {
        v.provenance = Provenance{};
    }
}

} // namespace arcs::artifact

#include "schema/validator.hpp"

namespace arcs::schema {

ValidationResult Validator::validate(
    const nlohmann::json& artifact,
    const std::string& schema_id,
    const SchemaRegistry& registry)
{
    ValidationResult result;
    result.valid = false;
    result.schema_id = schema_id;

    const SchemaEntry* schema_entry = registry.find_schema(schema_id);
    if (schema_entry == nullptr)
    {
        result.errors.push_back("Schema not found: " + schema_id);
        return result;
    }

    if (!artifact.is_object())
    {
        result.errors.push_back("Artifact must be a JSON object.");
        return result;
    }

    // Platz fuer spaetere echte JSON-Schema-Pruefung
    // z. B. artifact gegen schema_entry->document validieren

    result.valid = true;
    return result;
}

}
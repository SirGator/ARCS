/**
 * @file validator.hpp
 * @brief Declares Validator, which checks a JSON document against a
 *        registered JSON Schema.
 */

#pragma once

#include <nlohmann/json.hpp>
#include <string>

#include "schema/schema_types.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validation_result.hpp"

namespace arcs::schema {

/**
 * @brief Validates JSON documents against schemas held in a SchemaRegistry.
 */
class Validator {
public:
    /**
     * @brief Validates a JSON document against the named schema.
     * @param artifact The JSON document to validate.
     * @param schema_id The ID of the schema to validate against.
     * @param registry The registry to look up the schema in.
     * @return The validation result, including any validation errors. If
     *         the schema is not found, the result is invalid with a
     *         "Schema not found" error.
     */
    static ValidationResult validate(
        const nlohmann::json& artifact,
        const std::string& schema_id,
        const SchemaRegistry& registry);
};

} // namespace arcs::schema

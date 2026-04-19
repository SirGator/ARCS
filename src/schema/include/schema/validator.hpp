#pragma once

#include <nlohmann/json.hpp>
#include <string>

#include "schema/schema_types.hpp"
#include "schema/schema_registry.hpp"
#include "schema/validation_result.hpp"

namespace arcs::schema {

class Validator {
public:
    static ValidationResult validate(
        const nlohmann::json& artifact,
        const std::string& schema_id,
        const SchemaRegistry& registry);
};

} // namespace arcs::schema

#include "schema/validator.hpp"

#include <valijson/adapters/nlohmann_json_adapter.hpp>
#include <valijson/schema_parser.hpp>
#include <valijson/validation_results.hpp>
#include <valijson/validator.hpp>

#include <string>
#include <utility>

namespace arcs::schema {

ValidationResult Validator::validate(
    const nlohmann::json& artifact,
    const std::string& schema_id,
    const SchemaRegistry& registry)
{
    ValidationResult result;
    result.schema_id = schema_id;

    const SchemaEntry* schema_entry = registry.find_schema(schema_id);
    if (schema_entry == nullptr)
    {
        result.errors.push_back(ValidationError{"/", "Schema not found: " + schema_id});
        return result;
    }

    try
    {
        valijson::Schema schema;
        valijson::SchemaParser parser(valijson::SchemaParser::kDraft7);
        valijson::adapters::NlohmannJsonAdapter schema_adapter(schema_entry->document);
        parser.populateSchema(schema_adapter, schema);

        valijson::adapters::NlohmannJsonAdapter artifact_adapter(artifact);
        valijson::ValidationResults validation_results;
        valijson::Validator validator;

        result.valid = validator.validate(schema, artifact_adapter, &validation_results);

        for (const auto& error : validation_results)
        {
            ValidationError validation_error;
            validation_error.path = error.jsonPointer;
            validation_error.message = error.description;

            if (error.jsonPointer.empty())
            {
                validation_error.path = "/";
            }

            result.errors.push_back(std::move(validation_error));
        }
    }
    catch (const std::exception& e)
    {
        result.valid = false;
        result.errors.push_back(ValidationError{"/", std::string("Validation failed: ") + e.what()});
    }

    return result;
}

}

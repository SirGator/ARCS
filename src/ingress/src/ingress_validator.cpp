/**
 * @file ingress_validator.cpp
 * @brief Implements SchemaIngressValidator (schema-registry-backed
 *        validation) and MinimalIngressValidator (required-field-only
 *        validation) for ingress_event artifacts.
 */
#include "ingress/ingress_validator.hpp"

#include "artifact/json.hpp"
#include "schema/validator.hpp"

namespace arcs::ingress {

/**
 * @brief Constructs a schema-based validator bound to a schema registry.
 * @param registry Registry used to look up schema definitions.
 */
SchemaIngressValidator::SchemaIngressValidator(arcs::schema::SchemaRegistry& registry)
    : registry_(registry)
{}

/**
 * @brief Serializes the artifact to JSON and validates it against the
 *        schema identified by its schema_id, via the bound schema
 *        registry, concatenating any schema errors into the reason string.
 * @param ingress The artifact to validate.
 * @return Pass if valid, otherwise Fail with combined error details.
 */
ValidationResult SchemaIngressValidator::validate(const arcs::artifact::ArtifactVersion& ingress)
{
    ValidationResult result;

    const nlohmann::json artifact_json = ingress;
    const auto schema_result = arcs::schema::Validator::validate(
        artifact_json, ingress.schema_id, registry_);
    if (schema_result.valid) {
        result.status = ValidationStatus::Pass;
    } else {
        result.status = ValidationStatus::Fail;
        for (const auto& err : schema_result.errors) {
            if (!result.reason.empty()) {
                result.reason += "; ";
            }
            result.reason += err.path + ": " + err.message;
        }
    }

    return result;
}

/**
 * @brief Validates that the artifact is of type "ingress_event" and that
 *        its payload contains the required "raw_text" and "source_kind"
 *        fields, without consulting a schema registry.
 * @param ingress The artifact to validate.
 * @return Pass if all required fields are present, otherwise Fail with a
 *         reason identifying the missing/mismatched field.
 */
ValidationResult MinimalIngressValidator::validate(const arcs::artifact::ArtifactVersion& ingress)
{
    ValidationResult result;

    if (ingress.type != "ingress_event") {
        result.status = ValidationStatus::Fail;
        result.reason = "artifact type is not ingress_event";
        return result;
    }

    if (!ingress.payload.contains("raw_text")) {
        result.status = ValidationStatus::Fail;
        result.reason = "missing required field: raw_text";
        return result;
    }

    if (!ingress.payload.contains("source_kind")) {
        result.status = ValidationStatus::Fail;
        result.reason = "missing required field: source_kind";
        return result;
    }

    result.status = ValidationStatus::Pass;
    return result;
}

} // namespace arcs::ingress

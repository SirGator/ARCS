#include "ingress/ingress_validator.hpp"

#include "artifact/json.hpp"
#include "schema/validator.hpp"

namespace arcs::ingress {

SchemaIngressValidator::SchemaIngressValidator(arcs::schema::SchemaRegistry& registry)
    : registry_(registry)
{}

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

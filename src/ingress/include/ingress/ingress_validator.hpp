/**
 * @file ingress_validator.hpp
 * @brief Defines the validation stage of the ingress pipeline, which
 *        checks a normalized "ingress_event" artifact against a schema or
 *        against minimal required-field rules before routing.
 */
#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "schema/schema_registry.hpp"

namespace arcs::ingress {

/**
 * @brief Outcome status of validating an ingress_event artifact.
 */
enum class ValidationStatus {
    Pass,
    Fail,
    Unknown,
};

/**
 * @brief Result of validating an ingress_event artifact: the outcome
 *        status plus a human-readable reason when validation fails.
 */
struct ValidationResult {
    ValidationStatus status{ValidationStatus::Unknown};
    std::string reason;
};

// Interface: Validiert ingress_event gegen Schema.
/**
 * @brief Interface for validating an ingress_event artifact before it is
 *        routed further into the system.
 */
class IIngressValidator {
public:
    virtual ~IIngressValidator() = default;

    /**
     * @brief Validates the given ingress_event artifact.
     * @param ingress The artifact to validate.
     * @return The validation result.
     */
    virtual ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) = 0;
};

// Schema-basierte Validierung: prüft payload gegen registriertes Schema.
/**
 * @brief IIngressValidator implementation that validates an artifact's
 *        payload against its declared schema, using a SchemaRegistry to
 *        resolve the schema definition.
 */
class SchemaIngressValidator final : public IIngressValidator {
public:
    /**
     * @brief Constructs a schema-based validator bound to a schema registry.
     * @param registry Registry used to look up schema definitions. Must
     *        outlive this validator.
     */
    explicit SchemaIngressValidator(arcs::schema::SchemaRegistry& registry);

    /**
     * @brief Validates the artifact's payload against its schema_id,
     *        resolved via the bound schema registry.
     * @param ingress The artifact to validate.
     * @return Pass if the payload conforms to the schema, otherwise Fail
     *         with a combined reason string of all schema errors.
     */
    ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) override;

private:
    arcs::schema::SchemaRegistry& registry_;
};

// Minimal-Validierung: prüft nur Pflichtfelder.
/**
 * @brief IIngressValidator implementation that performs a lightweight
 *        check for required fields (artifact type and payload keys)
 *        without consulting a schema registry.
 */
class MinimalIngressValidator final : public IIngressValidator {
public:
    /**
     * @brief Validates that the artifact is of type "ingress_event" and
     *        that its payload contains the required "raw_text" and
     *        "source_kind" fields.
     * @param ingress The artifact to validate.
     * @return Pass if all required fields are present, otherwise Fail.
     */
    ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) override;
};

} // namespace arcs::ingress

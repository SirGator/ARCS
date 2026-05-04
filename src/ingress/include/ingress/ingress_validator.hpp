#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "schema/schema_registry.hpp"

namespace arcs::ingress {

enum class ValidationStatus {
    Pass,
    Fail,
    Unknown,
};

struct ValidationResult {
    ValidationStatus status{ValidationStatus::Unknown};
    std::string reason;
};

// Interface: Validiert ingress_event gegen Schema.
class IIngressValidator {
public:
    virtual ~IIngressValidator() = default;

    virtual ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) = 0;
};

// Schema-basierte Validierung: prüft payload gegen registriertes Schema.
class SchemaIngressValidator final : public IIngressValidator {
public:
    explicit SchemaIngressValidator(arcs::schema::SchemaRegistry& registry);

    ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) override;

private:
    arcs::schema::SchemaRegistry& registry_;
};

// Minimal-Validierung: prüft nur Pflichtfelder.
class MinimalIngressValidator final : public IIngressValidator {
public:
    ValidationResult validate(const arcs::artifact::ArtifactVersion& ingress) override;
};

} // namespace arcs::ingress

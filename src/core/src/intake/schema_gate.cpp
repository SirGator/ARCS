#include "core/intake/schema_gate.hpp"

#include "schema/validator.hpp"

namespace arcs::core::intake {

SchemaGate::SchemaGate(const arcs::schema::SchemaRegistry& registry)
    : registry_(registry)
{
}

SchemaGateResult SchemaGate::validate(const std::string& schema_id, const nlohmann::json& payload) const
{
    SchemaGateResult result;
    const auto validation = arcs::schema::Validator::validate(payload, schema_id, registry_);
    result.valid = validation.valid;
    for (const auto& error : validation.errors) {
        result.errors.push_back(error.path + ": " + error.message);
    }
    return result;
}

} // namespace arcs::core::intake

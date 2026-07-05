#pragma once

#include <string>

#include <nlohmann/json.hpp>

#include "schema/schema_registry.hpp"

namespace arcs::core::intake {

struct SchemaGateResult {
    bool valid{false};
    std::vector<std::string> errors;
};

class SchemaGate {
public:
    explicit SchemaGate(const arcs::schema::SchemaRegistry& registry);

    SchemaGateResult validate(const std::string& schema_id, const nlohmann::json& payload) const;

private:
    const arcs::schema::SchemaRegistry& registry_;
};

} // namespace arcs::core::intake

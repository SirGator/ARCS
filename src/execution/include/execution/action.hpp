#pragma once

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::execution {

struct ActionPayload {
    std::string action_id;
    std::string schema_id;
    std::string type;
    nlohmann::json params;
    std::vector<std::string> required_permissions;
    std::string safety_level;
    std::string idempotency_key;
};

struct Action {
    std::string artifact_id;
    std::string version_id;
    ActionPayload payload;
};

} // namespace arcs::execution

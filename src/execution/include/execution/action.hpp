#pragma once

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::execution {

struct ActionPayload {
    std::string action_id;
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

struct ActionRef {
    std::string artifact_id;
    std::string version_id;
};

struct ExecutionContext {
    std::string approval_id;
    std::string verification_id;
    bool approval_valid{false};
    bool verification_passed{false};
    std::vector<std::string> granted_permissions;
};

} // namespace arcs::execution

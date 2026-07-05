/**
 * @file action.hpp
 * @brief Core data structures for the execution module: the shape of an
 *        executable action (its payload and identity) and the execution
 *        context (approval/verification/permission state) that guards
 *        whether an action is allowed to run.
 */
#pragma once

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::execution {

/**
 * @brief The concrete parameters of an action to be executed, as produced
 *        by the materializer and consumed by an IExecutor.
 */
struct ActionPayload {
    std::string action_id;
    std::string type;
    nlohmann::json params;
    std::vector<std::string> required_permissions;
    std::string safety_level;
    std::string idempotency_key;
};

/**
 * @brief An action artifact: identifies a specific artifact/version pair
 *        and carries the payload describing what should be executed.
 */
struct Action {
    std::string artifact_id;
    std::string version_id;
    ActionPayload payload;
};

/**
 * @brief Lightweight reference to an action artifact (artifact_id +
 *        version_id), used to tag execution results back to their action.
 */
struct ActionRef {
    std::string artifact_id;
    std::string version_id;
};

/**
 * @brief Runtime context in which an action is executed, capturing the
 *        approval/verification state and the set of permissions granted
 *        for this execution. Consulted by the final guards and executors
 *        to decide whether an action may proceed.
 */
struct ExecutionContext {
    std::string approval_id;
    std::string verification_id;
    bool approval_valid{false};
    std::string approval_expires_at;
    bool verification_passed{false};
    std::vector<std::string> granted_permissions;
};

} // namespace arcs::execution

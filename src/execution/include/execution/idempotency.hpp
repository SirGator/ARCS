/**
 * @file idempotency.hpp
 * @brief Interface for storing and looking up execution results by action
 *        id, so that re-executing the same action (e.g. after a retry)
 *        returns the previously recorded result instead of running twice.
 */
#pragma once

#include <optional>
#include <string>

#include "execution/execution_result.hpp"

namespace arcs::execution {

/**
 * @brief Key-value store mapping an action id to the ExecutionResult
 *        produced the first time that action ran, enabling idempotent
 *        replay of already-executed actions.
 */
class IIdempotencyStore {
public:
    virtual ~IIdempotencyStore() = default;

    /**
     * @brief Check whether a result has already been recorded for an action.
     * @param action_id Identifier of the action to check.
     * @return True if a stored result exists for @p action_id.
     */
    virtual bool has(const std::string& action_id) const = 0;

    /**
     * @brief Retrieve the previously stored result for an action, if any.
     * @param action_id Identifier of the action to look up.
     * @return The stored ExecutionResult, or std::nullopt if none exists.
     */
    virtual std::optional<ExecutionResult> get(const std::string& action_id) const = 0;

    /**
     * @brief Record the execution result for an action.
     * @param action_id Identifier of the action that was executed.
     * @param result Execution result to store for future idempotent lookups.
     */
    virtual void put(const std::string& action_id, const ExecutionResult& result) = 0;
};

} // namespace arcs::execution

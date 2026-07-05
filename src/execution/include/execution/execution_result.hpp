/**
 * @file execution_result.hpp
 * @brief Defines the outcome of executing an action: status, exit code,
 *        error message, and an audit log trail, plus convenience factory
 *        functions for the common success/fail cases.
 */
#pragma once

#include <string>
#include <vector>

#include "execution/action.hpp"

namespace arcs::execution {

/**
 * @brief Terminal status of an action execution attempt.
 */
enum class ExecutionStatus {
    Success,
    Fail,
    Timeout,
    Cancelled
};

/**
 * @brief A single audit-log entry produced while executing an action.
 */
struct ExecutionLog {
    std::string message;
    std::string timestamp;
};

/**
 * @brief Result of executing (or attempting to execute) an action,
 *        including the action reference, status, exit code, error
 *        message on failure, and the accumulated log trail.
 */
struct ExecutionResult {
    ActionRef action_ref;
    ExecutionStatus status{ExecutionStatus::Fail};
    int exit_code{1};
    std::string error_message;
    std::vector<ExecutionLog> logs;

    /**
     * @brief Build a successful result for the given action reference.
     * @param ref Reference to the action that was executed.
     * @return An ExecutionResult with status Success and exit_code 0.
     */
    static ExecutionResult success(const ActionRef& ref)
    {
        ExecutionResult result{};
        result.action_ref = ref;
        result.status = ExecutionStatus::Success;
        result.exit_code = 0;
        return result;
    }

    /**
     * @brief Build a failed result for the given action reference.
     * @param ref Reference to the action that was executed.
     * @param message Human-readable description of the failure.
     * @return An ExecutionResult with status Fail, exit_code 1, and
     *         error_message set to @p message.
     */
    static ExecutionResult fail(const ActionRef& ref, const std::string& message)
    {
        ExecutionResult result{};
        result.action_ref = ref;
        result.status = ExecutionStatus::Fail;
        result.exit_code = 1;
        result.error_message = message;
        return result;
    }
};

} // namespace arcs::execution

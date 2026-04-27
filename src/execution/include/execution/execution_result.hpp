#pragma once

#include <string>
#include <vector>

#include "execution/action.hpp"

namespace arcs::execution {

enum class ExecutionStatus {
    Success,
    Fail,
    Timeout,
    Cancelled
};

struct ExecutionLog {
    std::string message;
    std::string timestamp;
};

struct ExecutionResult {
    ActionRef action_ref;
    ExecutionStatus status{ExecutionStatus::Fail};
    int exit_code{1};
    std::string error_message;
    std::vector<ExecutionLog> logs;

    static ExecutionResult success(const ActionRef& ref)
    {
        ExecutionResult result{};
        result.action_ref = ref;
        result.status = ExecutionStatus::Success;
        result.exit_code = 0;
        return result;
    }

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

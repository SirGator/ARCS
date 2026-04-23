#pragma once

#include <string>
#include <utility>
#include <vector>

namespace arcs::execution {

struct ActionRef {
    std::string artifact_id;
    std::string version_id;
};

enum class ExecutionStatus {
    Success,
    Fail,
    Cancelled,
    Timeout
};

struct ExecutionLog {
    std::string message;
    std::string timestamp;
};

struct ExecutionResult {
    ActionRef action_ref;
    ExecutionStatus status;
    std::vector<ExecutionLog> logs;
    int exit_code = 0;
    std::string error_message;

    static ExecutionResult success(const ActionRef& ref) {
        return ExecutionResult{
            .action_ref = ref,
            .status = ExecutionStatus::Success,
            .exit_code = 0
        };
    }

    static ExecutionResult fail(const ActionRef& ref, std::string err) {
        return ExecutionResult{
            .action_ref = ref,
            .status = ExecutionStatus::Fail,
            .exit_code = 1,
            .error_message = std::move(err)
        };
    }
};

} // namespace arcs::execution

#pragma once

#include <string>
#include <vector>

#include "execution/execution_adapter.hpp"

namespace arcs::execution {

struct Action;
struct ExecutionResult;

struct ExecutionContext {
    std::string approval_id;
    std::string verification_id;
    bool approval_valid;
    bool verification_passed;
    std::vector<std::string> granted_permissions;
};

using IExecutor = IExecutionAdapter;

} // namespace arcs::execution

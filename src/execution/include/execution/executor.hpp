#pragma once

#include <memory>
#include <string>
#include <vector>

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

class IExecutor {
public:
    virtual ~IExecutor() = default;

    virtual ExecutionResult execute(
        const Action& action,
        const ExecutionContext& ctx) = 0;

    virtual std::string handles_action_type() const = 0;
};

} // namespace arcs::execution

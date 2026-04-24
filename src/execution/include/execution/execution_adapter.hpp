#pragma once

#include <string>

namespace arcs::execution {

struct Action;
struct ExecutionContext;
struct ExecutionResult;

class IExecutionAdapter {
public:
    virtual ~IExecutionAdapter() = default;

    virtual ExecutionResult execute(
        const Action& action,
        const ExecutionContext& ctx) = 0;

    virtual std::string handles_action_type() const = 0;
};

} // namespace arcs::execution

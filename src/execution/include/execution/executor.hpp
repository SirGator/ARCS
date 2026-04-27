#pragma once

#include <string>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"

namespace arcs::execution {

class IExecutor {
public:
    virtual ~IExecutor() = default;

    virtual ExecutionResult execute(const Action& action, const ExecutionContext& ctx) = 0;
    virtual std::string handles_action_type() const = 0;
};

} // namespace arcs::execution

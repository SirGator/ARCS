#pragma once

#include "execution/action_handler_registry.hpp"

namespace arcs::execution {

class ActionDispatcher {
public:
    explicit ActionDispatcher(ActionHandlerRegistry& registry);

    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) const;

private:
    ActionHandlerRegistry& registry_;
};

} // namespace arcs::execution

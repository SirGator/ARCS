#include "execution/action_dispatcher.hpp"

namespace arcs::execution {

ActionDispatcher::ActionDispatcher(ActionHandlerRegistry& registry)
    : registry_(registry)
{
}

ExecutionResult ActionDispatcher::execute(const Action& action, const ExecutionContext& ctx) const
{
    const auto handler = registry_.find_handler(action.payload.type);
    if (!handler) {
        const ActionRef ref{.artifact_id = action.artifact_id, .version_id = action.version_id};
        return ExecutionResult::fail(ref, "no handler registered for action type: " + action.payload.type);
    }

    return handler->execute(action, ctx);
}

} // namespace arcs::execution

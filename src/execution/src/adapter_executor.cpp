#include "execution/adapter_executor.hpp"

#include <stdexcept>
#include <utility>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"

namespace arcs::execution {

AdapterExecutor::AdapterExecutor(AdapterRegistry& registry)
    : registry_(registry)
{
}

ExecutionResult AdapterExecutor::execute(const Action& action, const ExecutionContext& ctx) const
{
    const auto adapter = registry_.find_adapter(action.payload.type);
    if (!adapter) {
        const ActionRef ref{.artifact_id = action.artifact_id, .version_id = action.version_id};
        return ExecutionResult::fail(ref, "no adapter registered for action type: " + action.payload.type);
    }

    return adapter->execute(action, ctx);
}

} // namespace arcs::execution

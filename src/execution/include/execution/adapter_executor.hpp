#pragma once

#include <memory>

#include "execution/action.hpp"
#include "execution/adapter_registry.hpp"
#include "execution/executor.hpp"

namespace arcs::execution {

class AdapterExecutor {
public:
    explicit AdapterExecutor(AdapterRegistry& registry);

    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) const;

private:
    AdapterRegistry& registry_;
};

} // namespace arcs::execution

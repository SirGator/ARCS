#include "execution/adapter_registry.hpp"

namespace arcs::execution {

bool AdapterRegistry::register_adapter(std::shared_ptr<IExecutionAdapter> adapter)
{
    if (!adapter) {
        return false;
    }

    const auto type = adapter->handles_action_type();
    if (type.empty() || adapters_.contains(type)) {
        return false;
    }

    adapters_.emplace(type, std::move(adapter));
    return true;
}

std::shared_ptr<IExecutionAdapter> AdapterRegistry::find_adapter(const std::string& action_type) const
{
    const auto it = adapters_.find(action_type);
    if (it == adapters_.end()) {
        return nullptr;
    }

    return it->second;
}

bool AdapterRegistry::has_adapter(const std::string& action_type) const
{
    return adapters_.contains(action_type);
}

} // namespace arcs::execution

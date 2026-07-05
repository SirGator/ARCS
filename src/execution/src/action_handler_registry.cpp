#include "execution/action_handler_registry.hpp"

namespace arcs::execution {

bool ActionHandlerRegistry::register_handler(std::shared_ptr<IExecutor> handler)
{
    if (!handler) {
        return false;
    }

    const auto type = handler->handles_action_type();
    if (type.empty() || handlers_.contains(type)) {
        return false;
    }

    handlers_.emplace(type, std::move(handler));
    return true;
}

std::shared_ptr<IExecutor> ActionHandlerRegistry::find_handler(const std::string& action_type) const
{
    const auto it = handlers_.find(action_type);
    if (it == handlers_.end()) {
        return nullptr;
    }

    return it->second;
}

bool ActionHandlerRegistry::has_handler(const std::string& action_type) const
{
    return handlers_.contains(action_type);
}

} // namespace arcs::execution

#pragma once

#include <memory>
#include <string>
#include <unordered_map>

#include "execution/executor.hpp"

namespace arcs::execution {

class ActionHandlerRegistry {
public:
    bool register_handler(std::shared_ptr<IExecutor> handler);
    std::shared_ptr<IExecutor> find_handler(const std::string& action_type) const;
    bool has_handler(const std::string& action_type) const;

private:
    std::unordered_map<std::string, std::shared_ptr<IExecutor>> handlers_;
};

} // namespace arcs::execution

#pragma once

#include <memory>
#include <string>
#include <unordered_map>

#include "execution/execution_adapter.hpp"

namespace arcs::execution {

class AdapterRegistry {
public:
    bool register_adapter(std::shared_ptr<IExecutionAdapter> adapter);
    std::shared_ptr<IExecutionAdapter> find_adapter(const std::string& action_type) const;
    bool has_adapter(const std::string& action_type) const;

private:
    std::unordered_map<std::string, std::shared_ptr<IExecutionAdapter>> adapters_;
};

} // namespace arcs::execution

#pragma once

#include <memory>
#include <string>
#include <unordered_map>

#include "execution/executor.hpp"

namespace arcs::execution {

class IExecutionAdapter {
public:
    virtual ~IExecutionAdapter() = default;

    virtual std::string handles_action_type() const = 0;
    virtual ExecutionResult execute(const Action& action, const ExecutionContext& ctx) = 0;
};

class AdapterRegistry {
public:
    bool register_adapter(std::shared_ptr<IExecutionAdapter> adapter);
    std::shared_ptr<IExecutionAdapter> find_adapter(const std::string& action_type) const;
    bool has_adapter(const std::string& action_type) const;

private:
    std::unordered_map<std::string, std::shared_ptr<IExecutionAdapter>> adapters_;
};

} // namespace arcs::execution

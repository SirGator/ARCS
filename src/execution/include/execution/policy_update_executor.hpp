#pragma once

#include "execution/executor.hpp"

namespace arcs::store {
class IStore;
}

namespace arcs::execution {

class PolicyUpdateExecutor final : public IExecutor {
public:
    explicit PolicyUpdateExecutor(arcs::store::IStore& store);

    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) override;
    std::string handles_action_type() const override;

private:
    arcs::store::IStore& store_;
};

} // namespace arcs::execution

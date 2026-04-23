#pragma once

#include "execution/action.hpp"
#include "execution/executor.hpp"
#include "execution/idempotency.hpp"

namespace arcs::execution {

class ReportEmitExecutor final : public IExecutor {
public:
    explicit ReportEmitExecutor(IIdempotencyStore& idempotency_store);

    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) override;
    std::string handles_action_type() const override;

private:
    IIdempotencyStore& idempotency_store_;
};

} // namespace arcs::execution

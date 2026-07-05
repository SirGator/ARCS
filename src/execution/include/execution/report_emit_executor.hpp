/**
 * @file report_emit_executor.hpp
 * @brief Declares the executor responsible for handling "report_emit"
 *        actions, including final-guard checks and idempotent replay.
 */
#pragma once

#include "execution/executor.hpp"
#include "execution/idempotency.hpp"

namespace arcs::execution {

/**
 * @brief IExecutor implementation that handles "report_emit" actions.
 *        Runs final guard checks (verification, approval validity/
 *        expiry, granted permissions), consults an idempotency store to
 *        avoid re-running already-completed actions, and otherwise
 *        performs a deterministic, side-effect-free report emission.
 */
class ReportEmitExecutor final : public IExecutor {
public:
    /**
     * @brief Construct the executor with the idempotency store it uses to
     *        detect and replay previously-executed actions.
     * @param idempotency_store Store used to look up/record execution
     *                          results keyed by action id.
     */
    explicit ReportEmitExecutor(IIdempotencyStore& idempotency_store);

    /**
     * @brief Execute a report_emit action after passing all final guards.
     * @param action Action to execute; must be of type "report_emit".
     * @param ctx Execution context checked for verification/approval/
     *            permission state before execution proceeds.
     * @return Success result (with audit logs) on completion, a Cancelled
     *         result if blocked by a guard, or a Fail result on error or
     *         type mismatch. Idempotent replays return the stored result.
     */
    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) override;

    /** @brief Returns "report_emit", the action type this executor handles. */
    std::string handles_action_type() const override;

private:
    IIdempotencyStore& idempotency_store_;
};

} // namespace arcs::execution

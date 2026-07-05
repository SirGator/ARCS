/**
 * @file policy_update_executor.hpp
 * @brief Declares the executor responsible for applying "policy_update"
 *        actions: committing a new policy artifact version to the store.
 */
#pragma once

#include "execution/executor.hpp"

namespace arcs::store {
class IStore;
}

namespace arcs::execution {

/**
 * @brief IExecutor implementation that handles "policy_update" actions by
 *        building a new policy artifact version from the action's
 *        parameters and committing it (with a corresponding event) to the
 *        artifact store.
 */
class PolicyUpdateExecutor final : public IExecutor {
public:
    /**
     * @brief Construct the executor with the store it will commit to.
     * @param store Artifact/event store used to persist policy updates.
     */
    explicit PolicyUpdateExecutor(arcs::store::IStore& store);

    /**
     * @brief Execute a policy_update action: validate its parameters,
     *        build and commit a new policy artifact version.
     * @param action Action to execute; must be of type "policy_update".
     * @param ctx Execution context (currently unused by this executor).
     * @return Success result on commit, or a failure result describing
     *         the validation/commit error.
     */
    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) override;

    /** @brief Returns "policy_update", the action type this executor handles. */
    std::string handles_action_type() const override;

private:
    arcs::store::IStore& store_;
};

} // namespace arcs::execution

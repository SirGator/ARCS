/**
 * @file executor.hpp
 * @brief Defines the IExecutor interface implemented by concrete action
 *        executors (e.g. PolicyUpdateExecutor, ReportEmitExecutor).
 */
#pragma once

#include <string>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"

namespace arcs::execution {

/**
 * @brief Interface for a component that can execute a specific type of
 *        action within a given execution context.
 */
class IExecutor {
public:
    virtual ~IExecutor() = default;

    /**
     * @brief Execute the given action.
     * @param action Action to run, including its payload/parameters.
     * @param ctx Execution context carrying approval/verification/permission
     *            state used to gate execution.
     * @return The result of the execution attempt (success, failure, etc.).
     */
    virtual ExecutionResult execute(const Action& action, const ExecutionContext& ctx) = 0;

    /**
     * @brief The action type string this executor is responsible for
     *        (e.g. "policy_update", "report_emit").
     * @return The action type handled by this executor.
     */
    virtual std::string handles_action_type() const = 0;
};

} // namespace arcs::execution

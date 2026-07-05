/**
 * @file execution_service.hpp
 * @brief Executes an approved report action and captures its result.
 */

#pragma once

#include <optional>

#include "artifact/artifact.hpp"
#include "execution/action.hpp"
#include "execution/execution_result.hpp"

namespace arcs::core::services {

/**
 * @brief Outcome of executing an action: the raw execution result and, if
 *        one was produced, the artifact recording it.
 */
struct ExecutionOutcome {
    arcs::execution::ExecutionResult result;
    std::optional<arcs::artifact::ArtifactVersion> result_artifact;
};

/**
 * @brief Runs approved "report" actions against an execution context and
 *        records their results.
 */
class ExecutionService {
public:
    /**
     * @brief Executes the given report action.
     * @param action_artifact Action artifact to execute.
     * @param execution_context Context the action executes within.
     * @return The execution outcome, including any result artifact.
     */
    ExecutionOutcome execute_report_action(
        const arcs::artifact::ArtifactVersion& action_artifact,
        const arcs::execution::ExecutionContext& execution_context) const;
};

} // namespace arcs::core::services

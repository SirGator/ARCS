/**
 * @file execution_stage.hpp
 * @brief Flow stage that executes the approved, verified action.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for executing the final action and
 *        recording its result.
 */
class ExecutionStage {
public:
    /**
     * @brief Runs the execution stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

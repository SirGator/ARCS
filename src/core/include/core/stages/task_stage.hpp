/**
 * @file task_stage.hpp
 * @brief Flow stage that creates the task artifact anchoring the run.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for producing the task artifact from the
 *        interpreted input.
 */
class TaskStage {
public:
    /**
     * @brief Runs the task stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

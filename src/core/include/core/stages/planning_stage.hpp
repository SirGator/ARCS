/**
 * @file planning_stage.hpp
 * @brief Flow stage that plans a candidate option for the current task.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for producing the option to be verified and
 *        acted on.
 */
class PlanningStage {
public:
    /**
     * @brief Runs the planning stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

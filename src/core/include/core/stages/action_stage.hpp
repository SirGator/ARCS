/**
 * @file action_stage.hpp
 * @brief Flow stage that materializes an action candidate from the current
 *        option and policy.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for producing the action candidate to be
 *        verified and, if approved, executed.
 */
class ActionStage {
public:
    /**
     * @brief Runs the action stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

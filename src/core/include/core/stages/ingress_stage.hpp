/**
 * @file ingress_stage.hpp
 * @brief Flow stage that routes the raw input through ingress at the start
 *        of a run.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for ingesting the raw input and either
 *        accepting or quarantining it.
 */
class IngressStage {
public:
    /**
     * @brief Runs the ingress stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

/**
 * @file interpretation_stage.hpp
 * @brief Flow stage that interprets the accepted ingress event into a
 *        structured proposal.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for interpreting the ingested input into a
 *        proposal artifact.
 */
class InterpretationStage {
public:
    /**
     * @brief Runs the interpretation stage against the given runtime
     *        context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

/**
 * @file verification_stage.hpp
 * @brief Flow stage that verifies the current option/action against policy.
 */

#pragma once

#include "core/runtime/runtime_context.hpp"
#include "core/stages/stage_result.hpp"

namespace arcs::core::stages {

/**
 * @brief Flow stage responsible for running verification checks and
 *        recording the resulting report.
 */
class VerificationStage {
public:
    /**
     * @brief Runs the verification stage against the given runtime context.
     * @param context Mutable flow state to read from and update.
     * @return The stage outcome, indicating whether the flow should proceed.
     */
    StageResult run(runtime::RuntimeContext& context) const;
};

} // namespace arcs::core::stages

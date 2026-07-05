/**
 * @file stage_result.hpp
 * @brief Common result type returned by every flow stage, indicating
 *        whether the flow should continue, stop, or wait.
 */

#pragma once

namespace arcs::core::stages {

/** @brief Outcome status a stage can leave the flow in. */
enum class StageStatus {
    Continue,
    Blocked,
    Pending,
    Failed,
    Completed,
};

/**
 * @brief Status and reason returned by a stage after running, used by the
 *        runtime to decide whether to proceed to the next stage.
 */
struct StageResult {
    StageStatus status{StageStatus::Continue};
    std::string reason;

    /** @brief True if the flow should proceed to the next stage. */
    [[nodiscard]] bool continue_flow() const
    {
        return status == StageStatus::Continue;
    }
};

} // namespace arcs::core::stages

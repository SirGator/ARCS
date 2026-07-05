/**
 * @file task_state_reducer.hpp
 * @brief Reducer that folds a stream of artifacts into a TaskState.
 */
#pragma once

#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/reducer.hpp"
#include "reducer/task_state.hpp"

namespace arcs::reducer {

/**
 * @brief Reduces "option", "approval", and "execution_result" artifacts
 * into a single TaskState describing the task's overall status.
 */
class TaskStateReducer : public IReducer<TaskState> {
public:
    /**
     * @brief Folds the given artifacts into a TaskState.
     * @param artifacts Artifact history to reduce over.
     * @return The resulting TaskState.
     */
    TaskState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) override;
};

} // namespace arcs::reducer

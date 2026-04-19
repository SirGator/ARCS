#pragma once

#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/reducer.hpp"
#include "reducer/task_state.hpp"

namespace arcs::reducer {

class TaskStateReducer : public IReducer<TaskState> {
public:
    TaskState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) override;
};

} // namespace arcs::reducer

/**
 * @file approval_state_reducer.hpp
 * @brief Reducer that folds a stream of artifacts into an ApprovalState.
 */
#pragma once

#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/approval_state.hpp"
#include "reducer/reducer.hpp"

namespace arcs::reducer {

/**
 * @brief Reduces "approval" type artifacts into a single ApprovalState
 * reflecting the most recent decision and policy reference.
 */
class ApprovalStateReducer : public IReducer<ApprovalState> {
public:
    /**
     * @brief Folds the given artifacts into an ApprovalState.
     * @param artifacts Artifact history to reduce over.
     * @return The resulting ApprovalState.
     */
    ApprovalState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) override;
};

} // namespace arcs::reducer

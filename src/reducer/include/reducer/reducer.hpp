/**
 * @file reducer.hpp
 * @brief Generic reducer interface used to fold artifact histories into a
 * derived state type.
 */
#pragma once

#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::reducer {

/**
 * @brief Interface for a pure reducer that derives a state of type TState
 * from an ordered list of artifacts.
 * @tparam TState The state type produced by the reduction.
 */
template<typename TState>
class IReducer {
public:
    virtual ~IReducer() = default;

    /**
     * @brief Folds the given artifacts into a TState value.
     * @param artifacts Artifact history to reduce over.
     * @return The resulting state.
     */
    virtual TState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) = 0;
};

} // namespace arcs::reducer

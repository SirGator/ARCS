#pragma once

#include <vector>

#include "artifact/artifact.hpp"

namespace arcs::reducer {

template<typename TState>
class IReducer {
public:
    virtual ~IReducer() = default;

    virtual TState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) = 0;
};

} // namespace arcs::reducer

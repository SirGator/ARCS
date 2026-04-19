#pragma once

#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/approval_state.hpp"
#include "reducer/reducer.hpp"

namespace arcs::reducer {

class ApprovalStateReducer : public IReducer<ApprovalState> {
public:
    ApprovalState reduce(const std::vector<arcs::artifact::ArtifactVersion>& artifacts) override;
};

} // namespace arcs::reducer

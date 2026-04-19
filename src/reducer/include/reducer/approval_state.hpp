#pragma once
#include <string>

namespace arcs::reducer {

struct ApprovalState {
    std::string decision;
    std::string policy_ref;
    bool valid{false};
};

} // namespace arcs::reducer

/**
 * @file approval_state.hpp
 * @brief Defines the reduced approval state derived from a task's artifact
 * history.
 */
#pragma once
#include <string>

namespace arcs::reducer {

/**
 * @brief Result of reducing approval artifacts: the latest decision, the
 * policy it was made against, and whether that decision is currently valid
 * (i.e. approved).
 */
struct ApprovalState {
    std::string decision;
    std::string policy_ref;
    bool valid{false};
};

} // namespace arcs::reducer

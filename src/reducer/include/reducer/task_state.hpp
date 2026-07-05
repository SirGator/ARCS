/**
 * @file task_state.hpp
 * @brief Defines the reduced lifecycle state of a task.
 */
#pragma once
#include <string>
#include <vector>

namespace arcs::reducer {

/**
 * @brief Reduced view of a task's lifecycle: its current status
 * ("draft"/"blocked"/"approved"/"executed") plus the option and approval
 * artifacts contributing to it.
 */
struct TaskState {
    std::string status;
    std::vector<std::string> option_ids;
    std::vector<std::string> approval_ids;
};

} // namespace arcs::reducer

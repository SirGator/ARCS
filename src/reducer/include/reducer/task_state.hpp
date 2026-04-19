#pragma once
#include <string>
#include <vector>

namespace arcs::reducer {

struct TaskState {
    std::string status;
    std::vector<std::string> option_ids;
    std::vector<std::string> approval_ids;
};

} // namespace arcs::reducer

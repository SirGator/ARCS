#pragma once

#include <string>

#include "execution/executor.hpp"

namespace arcs::execution {

bool is_revoked(const ExecutionContext& ctx, std::string& reason);

} // namespace arcs::execution

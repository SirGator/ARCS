#pragma once

#include <string>

namespace arcs::execution {

struct ExecutionContext;

bool is_revoked(const ExecutionContext& ctx, std::string& reason);

} // namespace arcs::execution

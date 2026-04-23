#include "execution/executor.hpp"

#include "execution/revocation_check.hpp"

namespace arcs::execution {

bool is_revoked(const ExecutionContext& ctx, std::string& reason) {
  if (!ctx.approval_valid) {
    reason = "Execution revoked or approval no longer valid.";
    return true;
  }

  return false;
}

} // namespace arcs::execution

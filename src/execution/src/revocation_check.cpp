#include "execution/executor.hpp"

#include "execution/revocation_check.hpp"

/**
 * @file revocation_check.cpp
 * @brief Implements is_revoked(), which reports whether an execution
 *        context's approval has been revoked or is otherwise no longer
 *        valid.
 */

namespace arcs::execution {

/**
 * @brief Determine whether execution should be treated as revoked.
 * @param ctx Execution context to inspect (uses approval_valid).
 * @param reason Output parameter set to a human-readable explanation when
 *               revocation is detected.
 * @return True if approval_valid is false (execution is revoked).
 */
bool is_revoked(const ExecutionContext& ctx, std::string& reason) {
  if (!ctx.approval_valid) {
    reason = "Execution revoked or approval no longer valid.";
    return true;
  }

  return false;
}

} // namespace arcs::execution

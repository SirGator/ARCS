/**
 * @file revocation_check.hpp
 * @brief Declares a helper to check whether an execution context's
 *        approval has been revoked or is no longer valid.
 */
#pragma once

#include <string>

#include "execution/executor.hpp"

namespace arcs::execution {

/**
 * @brief Check whether execution should be treated as revoked based on
 *        the current approval state.
 * @param ctx Execution context to inspect (uses approval_valid).
 * @param reason Output parameter set to a human-readable explanation when
 *               the check reports revocation.
 * @return True if execution is revoked (approval no longer valid).
 */
bool is_revoked(const ExecutionContext& ctx, std::string& reason);

} // namespace arcs::execution

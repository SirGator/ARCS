/**
 * @file final_guards.cpp
 * @brief Implements the last line of defense before an action executes:
 *        verification-passed, approval-validity, and required-permission
 *        checks, plus a combined run_final_guards() entry point.
 */
#include <string>
#include <vector>

#include "execution/action.hpp"
#include "execution/executor.hpp"

namespace arcs::execution {

namespace {

/**
 * @brief Check whether a required permission is present among granted ones.
 * @param granted_permissions Permissions available in the execution context.
 * @param required_permission Permission to look for.
 * @return True if @p required_permission is present in @p granted_permissions.
 */
bool has_permission(
    const std::vector<std::string>& granted_permissions,
    const std::string& required_permission) {
  for (const auto& permission : granted_permissions) {
    if (permission == required_permission) {
      return true;
    }
  }
  return false;
}

} // namespace

/**
 * @brief Guard: ensure the execution context's verification has passed.
 * @param ctx Execution context to check.
 * @param error Output parameter set to a description of the failure.
 * @return True if verification_passed is true; false otherwise.
 */
bool check_verification_passed(const ExecutionContext& ctx, std::string& error) {
  if (!ctx.verification_passed) {
    error = "Execution blocked: verification status is not pass.";
    return false;
  }
  return true;
}

/**
 * @brief Guard: ensure the execution context's approval is valid.
 * @param ctx Execution context to check.
 * @param error Output parameter set to a description of the failure.
 * @return True if approval_valid is true; false otherwise.
 */
bool check_approval_valid(const ExecutionContext& ctx, std::string& error) {
  if (!ctx.approval_valid) {
    error = "Execution blocked: approval is invalid or expired.";
    return false;
  }
  return true;
}

/**
 * @brief Guard: ensure all permissions required by the action are granted
 *        in the execution context.
 * @param action Action whose required_permissions must all be granted.
 * @param ctx Execution context providing the granted permissions.
 * @param error Output parameter set to a description of the first missing
 *              permission found.
 * @return True if every required permission is granted; false otherwise.
 */
bool check_permissions(const Action& action, const ExecutionContext& ctx, std::string& error) {
  for (const auto& required : action.payload.required_permissions) {
    if (!has_permission(ctx.granted_permissions, required)) {
      error = "Execution blocked: missing required permission: " + required;
      return false;
    }
  }
  return true;
}

/**
 * @brief Run all final guards (verification, approval, permissions) in
 *        sequence, stopping at the first failure.
 * @param action Action about to be executed.
 * @param ctx Execution context to validate against.
 * @param error Output parameter set to a description of the first guard
 *              that failed, if any.
 * @return True if all guards pass; false if any guard blocks execution.
 */
bool run_final_guards(const Action& action, const ExecutionContext& ctx, std::string& error) {
  if (!check_verification_passed(ctx, error)) {
    return false;
  }

  if (!check_approval_valid(ctx, error)) {
    return false;
  }

  if (!check_permissions(action, ctx, error)) {
    return false;
  }

  return true;
}

} // namespace arcs::execution

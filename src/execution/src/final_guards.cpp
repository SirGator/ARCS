#include <string>
#include <vector>

#include "execution/action.hpp"
#include "execution/executor.hpp"

namespace arcs::execution {

namespace {

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

bool check_verification_passed(const ExecutionContext& ctx, std::string& error) {
  if (!ctx.verification_passed) {
    error = "Execution blocked: verification status is not pass.";
    return false;
  }
  return true;
}

bool check_approval_valid(const ExecutionContext& ctx, std::string& error) {
  if (!ctx.approval_valid) {
    error = "Execution blocked: approval is invalid or expired.";
    return false;
  }
  return true;
}

bool check_permissions(const Action& action, const ExecutionContext& ctx, std::string& error) {
  for (const auto& required : action.payload.required_permissions) {
    if (!has_permission(ctx.granted_permissions, required)) {
      error = "Execution blocked: missing required permission: " + required;
      return false;
    }
  }
  return true;
}

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

#include "execution/report_emit_executor.hpp"

#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>
#include <chrono>
#include <ctime>
#include <iomanip>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"
#include "execution/idempotency.hpp"

/**
 * @file report_emit_executor.cpp
 * @brief Implements ReportEmitExecutor::execute: runs final guard checks
 *        (verification, approval validity/expiry, permissions), consults
 *        the idempotency store for replay, and otherwise performs a
 *        deterministic report-emission (MVP: no shell/network access).
 */

namespace arcs::execution {

namespace {

// Hilfsfunktion für Audit-Logs
/**
 * @brief Build an ExecutionLog entry.
 * @param message Log message text.
 * @param timestamp Optional timestamp string (defaults to empty).
 * @return The constructed ExecutionLog.
 */
ExecutionLog make_log(std::string message, std::string timestamp = "") {
  return ExecutionLog{
      .message = std::move(message),
      .timestamp = std::move(timestamp),
  };
}

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

/**
 * @brief Get the current UTC time formatted as an ISO-8601 timestamp.
 * @return Timestamp string in "%Y-%m-%dT%H:%M:%SZ" format.
 */
std::string utc_now()
{
  const auto now = std::chrono::system_clock::now();
  const auto now_time_t = std::chrono::system_clock::to_time_t(now);
  std::tm tm{};

#if defined(_WIN32)
  gmtime_s(&tm, &now_time_t);
#else
  gmtime_r(&now_time_t, &tm);
#endif

  std::ostringstream out;
  out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
  return out.str();
}

/**
 * @brief Check whether the context's approval has passed its expiry time.
 * @param ctx Execution context whose approval_expires_at is compared to now.
 * @return True if approval_expires_at is set and is in the past.
 */
bool approval_is_expired(const ExecutionContext& ctx)
{
  return !ctx.approval_expires_at.empty() && utc_now() > ctx.approval_expires_at;
}

} // namespace

/** @brief Constructs the executor, retaining a reference to the idempotency store. */
ReportEmitExecutor::ReportEmitExecutor(IIdempotencyStore& idempotency_store)
    : idempotency_store_(idempotency_store)
{
}

/**
 * @brief Executes a "report_emit" action after passing all final guards
 *        (verification passed, approval valid and unexpired, required
 *        permissions granted). Replays a previously stored result if the
 *        action id is already known to the idempotency store; otherwise
 *        performs the (deterministic, side-effect-free) report emission
 *        and records the result for future replay.
 * @param action Action to execute; payload.type must equal
 *               handles_action_type() ("report_emit").
 * @param ctx Execution context providing verification/approval/permission
 *            state.
 * @return Success result with audit logs on completion; a Cancelled
 *         result if blocked by a guard; a Fail result on type mismatch or
 *         unexpected exception; or the replayed prior result if the
 *         action was already executed.
 */
ExecutionResult ReportEmitExecutor::execute(
    const Action& action,
    const ExecutionContext& ctx)
{
    const std::string& action_id = action.payload.action_id;

    // Referenz für Resultat aufbauen
    ActionRef ref{
        .artifact_id = action.artifact_id,
        .version_id = action.version_id,
    };

    // 2) Final Guards
    if (!ctx.verification_passed) {
      auto result = ExecutionResult::fail(
          ref, "Execution blocked: verification status is not pass.");
      result.status = ExecutionStatus::Cancelled;
      result.logs.push_back(
          make_log("Blocked by final guard: verification_passed=false."));
      return result;
    }

    if (!ctx.approval_valid) {
      auto result = ExecutionResult::fail(
          ref, "Execution blocked: approval is invalid or expired.");
      result.status = ExecutionStatus::Cancelled;
      result.logs.push_back(
          make_log("Blocked by final guard: approval_valid=false."));
      return result;
    }

    if (approval_is_expired(ctx)) {
      auto result = ExecutionResult::fail(
          ref, "Execution blocked: approval is invalid or expired.");
      result.status = ExecutionStatus::Cancelled;
      result.logs.push_back(
          make_log("Blocked by final guard: approval_expires_at is in the past."));
      return result;
    }

    for (const auto& required : action.payload.required_permissions) {
      if (!has_permission(ctx.granted_permissions, required)) {
        auto result = ExecutionResult::fail(
            ref, "Execution blocked: missing required permission: " + required);
        result.status = ExecutionStatus::Cancelled;
        result.logs.push_back(
            make_log("Blocked by final guard: missing permission '" + required + "'."));
        return result;
      }
    }

    // 3) Idempotenz erst nach erfolgreichem Guard-Check prüfen.
    if (auto existing = idempotency_store_.get(action_id); existing.has_value()) {
      ExecutionResult replayed = *existing;
      replayed.logs.push_back(
          make_log("Idempotent replay: existing execution_result returned."));
      return replayed;
    }

    // 4) Executor kann nur report_emit
    if (action.payload.type != handles_action_type()) {
      auto result = ExecutionResult::fail(
          ref,
          "Executor/action mismatch: expected action type '" +
              handles_action_type() + "', got '" + action.payload.type + "'.");
      result.logs.push_back(make_log("Rejected: unsupported action type."));
      return result;
    }

    // 5) Report-Logik (MVP: deterministisch, ohne Shell/Netz)
    try {
      std::ostringstream log_stream;
      log_stream << "ReportEmitExecutor executed action_id=" << action_id;

      if (action.payload.params.contains("format") &&
          action.payload.params["format"].is_string()) {
        log_stream << ", format=" << action.payload.params["format"].get<std::string>();
      }

      if (action.payload.params.contains("sections") &&
          action.payload.params["sections"].is_array()) {
        log_stream << ", sections=" << action.payload.params["sections"].size();
      }

      ExecutionResult result = ExecutionResult::success(ref);
      result.logs.push_back(make_log("Pre-flight checks passed."));
      result.logs.push_back(make_log(log_stream.str()));
      result.logs.push_back(make_log("Report generated successfully."));

      // 6) Erst nach erfolgreicher Ausführung speichern
      idempotency_store_.put(action_id, result);
      return result;

    } catch (const std::exception& ex) {
      ExecutionResult result = ExecutionResult::fail(
          ref, std::string("Report execution failed: ") + ex.what());
      result.logs.push_back(make_log("Execution threw exception."));
      return result;
    } catch (...) {
      ExecutionResult result = ExecutionResult::fail(
          ref, "Report execution failed: unknown exception.");
      result.logs.push_back(make_log("Execution threw unknown exception."));
      return result;
    }
}

/** @brief Returns "report_emit", the action type this executor handles. */
std::string ReportEmitExecutor::handles_action_type() const
{
    return "report_emit";
}

} // namespace arcs::execution

#include "execution/report_emit_executor.hpp"

#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"
#include "execution/idempotency.hpp"

namespace arcs::execution {

namespace {

// Hilfsfunktion für Audit-Logs
ExecutionLog make_log(std::string message, std::string timestamp = "") {
  return ExecutionLog{
      .message = std::move(message),
      .timestamp = std::move(timestamp),
  };
}

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

ReportEmitExecutor::ReportEmitExecutor(IIdempotencyStore& idempotency_store)
    : idempotency_store_(idempotency_store)
{
}

ExecutionResult ReportEmitExecutor::execute(
    const Action& action,
    const ExecutionContext& ctx)
{
    const std::string& action_id = action.payload.action_id;

    // 1) Idempotenz zuerst prüfen
    if (auto existing = idempotency_store_.get(action_id); existing.has_value()) {
      ExecutionResult replayed = *existing;
      replayed.logs.push_back(
          make_log("Idempotent replay: existing execution_result returned."));
      return replayed;
    }

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

    // 3) Executor kann nur report_emit
    if (action.payload.type != handles_action_type()) {
      auto result = ExecutionResult::fail(
          ref,
          "Executor/action mismatch: expected action type '" +
              handles_action_type() + "', got '" + action.payload.type + "'.");
      result.logs.push_back(make_log("Rejected: unsupported action type."));
      return result;
    }

    // 4) Report-Logik (MVP: deterministisch, ohne Shell/Netz)
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

      // 5) Erst nach erfolgreicher Ausführung speichern
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

std::string ReportEmitExecutor::handles_action_type() const
{
    return "report_emit";
}

} // namespace arcs::execution

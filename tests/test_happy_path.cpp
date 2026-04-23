#include <cassert>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"
#include "execution/executor.hpp"
#include "execution/idempotency.hpp"

// Falls du final_guards / revocation_check direkt im Executor nutzt,
// müssen sie hier nicht separat eingebunden werden.

namespace arcs::execution {

// --- Minimaler In-Memory-Idempotency-Store für den Test ---
class InMemoryIdempotencyStore final : public IIdempotencyStore {
public:
  bool has(const std::string& action_id) const override {
    return results_.find(action_id) != results_.end();
  }

  std::optional<ExecutionResult> get(
      const std::string& action_id) const override {
    auto it = results_.find(action_id);
    if (it == results_.end()) {
      return std::nullopt;
    }
    return it->second;
  }

  void put(
      const std::string& action_id,
      const ExecutionResult& result) override {
    results_[action_id] = result;
  }

private:
  std::unordered_map<std::string, ExecutionResult> results_;
};

} // namespace arcs::execution

#include "execution/report_emit_executor.hpp"

int main() {
  using namespace arcs::execution;

  // 1. Idempotency Store
  InMemoryIdempotencyStore idempotency_store;

  // 2. Executor
  ReportEmitExecutor executor(idempotency_store);

  // 3. Gültige Action aufbauen
  Action action{
      .artifact_id = "a_action_01",
      .version_id = "v_action_01",
      .payload =
          ActionPayload{
              .action_id = "x_action_01",
              .type = "report_emit",
              .params = {
                  {"format", "pdf"},
                  {"sections", {"summary", "risks"}}
              },
              .required_permissions = {"exec:report_emit"},
          },
  };

  // 4. Gültiger ExecutionContext
  ExecutionContext ctx{
      .approval_id = "a_approval_01",
      .verification_id = "a_verification_01",
      .approval_valid = true,
      .verification_passed = true,
      .granted_permissions = {"exec:report_emit"},
  };

  // 5. Ausführen
  ExecutionResult result = executor.execute(action, ctx);

  // 6. Assertions
  assert(result.action_ref.artifact_id == "a_action_01");
  assert(result.action_ref.version_id == "v_action_01");
  assert(result.status == ExecutionStatus::Success);
  assert(result.exit_code == 0);
  assert(result.error_message.empty());
  assert(!result.logs.empty());

  std::cout << "[PASS] Happy Path: execution_result=success\n";
  return 0;
}

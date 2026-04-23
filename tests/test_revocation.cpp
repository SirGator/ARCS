#include <cassert>
#include <iostream>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include "execution/action.hpp"
#include "execution/execution_result.hpp"
#include "execution/executor.hpp"
#include "execution/idempotency.hpp"
#include "execution/report_emit_executor.hpp"

namespace arcs::execution {

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

int main() {
  using namespace arcs::execution;

  InMemoryIdempotencyStore idempotency_store;
  ReportEmitExecutor executor(idempotency_store);

  Action action{
      .artifact_id = "a_action_01",
      .version_id = "v_action_01",
      .payload =
          ActionPayload{
              .action_id = "x_action_revoked",
              .type = "report_emit",
              .params = {
                  {"format", "pdf"},
                  {"sections", {"summary", "risks"}}
              },
              .required_permissions = {"exec:report_emit"},
          },
  };

  // approval_valid = false modelliert hier Revocation / nicht mehr gültige Freigabe
  ExecutionContext ctx{
      .approval_id = "a_approval_01",
      .verification_id = "a_verification_01",
      .approval_valid = false,
      .verification_passed = true,
      .granted_permissions = {"exec:report_emit"},
  };

  ExecutionResult result = executor.execute(action, ctx);

  assert(result.action_ref.artifact_id == "a_action_01");
  assert(result.action_ref.version_id == "v_action_01");
  assert(result.status == ExecutionStatus::Cancelled);
  assert(!result.error_message.empty());
  assert(!result.logs.empty());

  // Wichtiger Punkt:
  // Revoked/invalid approval darf NICHT als ausgeführt gespeichert werden
  assert(!idempotency_store.has("x_action_revoked"));

  std::cout << "[PASS] Revocation: revoked approval blocks execution\n";
  return 0;
}

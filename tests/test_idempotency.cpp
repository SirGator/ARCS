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
              .action_id = "x_action_same",
              .type = "report_emit",
              .params = {
                  {"format", "pdf"},
                  {"sections", {"summary", "risks"}}
              },
              .required_permissions = {"exec:report_emit"},
          },
  };

  ExecutionContext ctx{
      .approval_id = "a_approval_01",
      .verification_id = "a_verification_01",
      .approval_valid = true,
      .verification_passed = true,
      .granted_permissions = {"exec:report_emit"},
  };

  // Erste Ausführung
  ExecutionResult first = executor.execute(action, ctx);

  // Zweite Ausführung mit derselben action_id
  ExecutionResult second = executor.execute(action, ctx);

  assert(first.action_ref.artifact_id == "a_action_01");
  assert(first.action_ref.version_id == "v_action_01");
  assert(first.status == ExecutionStatus::Success);
  assert(first.exit_code == 0);

  assert(second.action_ref.artifact_id == "a_action_01");
  assert(second.action_ref.version_id == "v_action_01");
  assert(second.status == ExecutionStatus::Success);
  assert(second.exit_code == 0);

  // Wichtiger Kern des Tests:
  // dieselbe action_id wurde gespeichert und wiederverwendet
  assert(idempotency_store.has("x_action_same"));

  auto stored = idempotency_store.get("x_action_same");
  assert(stored.has_value());
  assert(stored->status == ExecutionStatus::Success);

  // Optional: zweite Ausführung sollte mindestens so viele Logs haben
  // wie die erste, oft sogar eins mehr wegen "Idempotent replay"
  assert(second.logs.size() >= first.logs.size());

  std::cout << "[PASS] Idempotency: same action_id returns stored result\n";
  return 0;
}

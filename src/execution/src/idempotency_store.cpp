#include "execution/idempotency.hpp"

#include <optional>
#include <string>
#include <unordered_map>
#include <utility>

#include "execution/execution_result.hpp"

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

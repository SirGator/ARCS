/**
 * @file idempotency_store.cpp
 * @brief Provides InMemoryIdempotencyStore, a simple in-process
 *        implementation of IIdempotencyStore backed by an unordered_map.
 *        Intended for MVP/testing use; results are not persisted across
 *        process restarts.
 */
#include "execution/idempotency.hpp"

#include <optional>
#include <string>
#include <unordered_map>
#include <utility>

#include "execution/execution_result.hpp"

namespace arcs::execution {

/**
 * @brief IIdempotencyStore implementation that keeps execution results in
 *        an in-memory hash map keyed by action id.
 */
class InMemoryIdempotencyStore final : public IIdempotencyStore {
public:
  /** @brief Returns whether a result has been recorded for the given action id. */
  bool has(const std::string& action_id) const override {
    return results_.find(action_id) != results_.end();
  }

  /** @brief Returns the stored result for the given action id, if any. */
  std::optional<ExecutionResult> get(
      const std::string& action_id) const override {
    auto it = results_.find(action_id);
    if (it == results_.end()) {
      return std::nullopt;
    }
    return it->second;
  }

  /** @brief Stores (or overwrites) the result for the given action id. */
  void put(
      const std::string& action_id,
      const ExecutionResult& result) override {
    results_[action_id] = result;
  }

private:
  std::unordered_map<std::string, ExecutionResult> results_;
};

} // namespace arcs::execution

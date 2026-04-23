#pragma once

#include <optional>
#include <string>

#include "execution/execution_result.hpp"

namespace arcs::execution {

class IIdempotencyStore {
public:
    virtual ~IIdempotencyStore() = default;

    virtual bool has(const std::string& action_id) const = 0;
    virtual std::optional<ExecutionResult> get(const std::string& action_id) const = 0;
    virtual void put(const std::string& action_id, const ExecutionResult& result) = 0;
};

} // namespace arcs::execution

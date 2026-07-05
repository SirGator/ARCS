/**
 * @file flow_result.hpp
 * @brief Result types describing the outcome of running the core flow,
 *        including any pending human-in-the-loop state.
 */

#pragma once

#include <optional>
#include <string>
#include <vector>

namespace arcs::core {

enum class FlowStatus {
    Blocked,
    Completed,
    Pending,
    Failed,
};

enum class DiagnosticSeverity {
    Info,
    Error,
};

struct Diagnostic {
    std::string code;
    DiagnosticSeverity severity{DiagnosticSeverity::Info};
    std::string message;
    std::string stage;
    std::string artifact_id;
};

/**
 * @brief Identifies a flow that is paused awaiting an external action (e.g.
 *        approval or permission grant), and the artifact it is waiting on.
 */
struct PendingState {
    std::string kind;
    std::string artifact_id;
};

/**
 * @brief Full outcome of a flow run as a typed core result.
 */
struct FlowResult {
    std::string input;
    FlowStatus status{FlowStatus::Blocked};
    std::string reason;
    std::optional<PendingState> pending;
    std::vector<Diagnostic> diagnostics;
};

inline bool is_not_blocked(const FlowResult& result)
{
    return result.status == FlowStatus::Completed;
}

} // namespace arcs::core

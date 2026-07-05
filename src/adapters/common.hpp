/**
 * @file common.hpp
 * @brief Shared types used across all ARCS adapter implementations (input,
 *        output, execution, interpretation, etc.): the draft artifact alias
 *        and the small result structs adapters use to report local
 *        validation, core submission, and capability-check outcomes, plus
 *        the common IAdapter marker interface.
 */
#pragma once

#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace arcs::core::intake {
struct AdapterSubmission;
}

namespace arcs::adapters {

/** @brief Adapter-local draft handed to the core intake layer, not a committed artifact. */
using DraftArtifact = arcs::core::intake::AdapterSubmission;

/** @brief Outcome of an adapter's own local validation of a draft artifact, prior to core submission. */
struct LocalValidationResult {
    bool ok{false};
    nlohmann::json diagnostics = nlohmann::json::object();
};

/** @brief Outcome of submitting a draft artifact to the core (e.g. via ingress). */
struct CoreSubmissionResult {
    bool accepted{false};
    nlohmann::json diagnostics = nlohmann::json::object();
};

/** @brief Outcome of checking whether an adapter is permitted to perform a given capability/action. */
struct CapabilityCheckResult {
    bool allowed{false};
    nlohmann::json diagnostics = nlohmann::json::object();
};

/** @brief Final top-level adapter classes recognized by the architecture. */
enum class AdapterKind {
    Input,
    Interpretation,
    Reasoning,
    Llm,
    ExternalState,
    Database,
    Output,
};

/** @brief Stable technical metadata exposed by every adapter. */
struct AdapterInfo {
    std::string id;
    AdapterKind kind{AdapterKind::Input};
    std::vector<std::string> capabilities;
};

/** @brief Minimal health state exposed by every adapter. */
struct AdapterHealth {
    bool ok{true};
    std::string status{"ok"};
};

/** @brief Common base interface implemented by all concrete ARCS adapters. */
class IAdapter {
public:
    virtual ~IAdapter() = default;
    virtual AdapterInfo info() const = 0;
    virtual AdapterHealth health() const = 0;
};

} // namespace arcs::adapters

/**
 * @file flow.hpp
 * @brief Legacy/demo entry points for running the end-to-end text flow, plus
 *        the flags that toggle demo-mode shortcuts.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"
#include "core/flow_result.hpp"
#include "interpretation/config.hpp"

namespace arcs::core {

/**
 * @brief Flags controlling demo-mode shortcuts for the flow (bypassing
 *        approval/permission checks or forcing simulated policy drift).
 */
struct FlowOptions {
    bool enable_demo_controls{false};
    bool demo_approval_granted{false};
    bool demo_permission_granted{false};
    bool demo_policy_drift{false};
};

/**
 * @brief Runs the full ingress-to-decision text flow for a raw text input.
 *        Compatibility/demo facade; new compositions should prefer
 *        `core::runtime::CoreRuntime` directly.
 * @param input Raw text input to process.
 * @param interpretation_config Optional interpretation API configuration;
 *        pass nullptr to use the default.
 * @param options Demo-mode flow options.
 * @return Rendered summary of the flow result.
 */
std::string run_text_flow(
    const std::string& input,
    const arcs::interpretation::InterpretationApiConfig* interpretation_config = nullptr,
    const FlowOptions& options = {});

/**
 * @brief Runs the full flow starting from an already-ingested artifact.
 *        Compatibility/demo facade; persistent or store-aware callers should
 *        use `core::runtime::CoreRuntime`.
 * @param input_artifact Artifact version to use as the flow's input.
 * @param options Demo-mode flow options.
 * @return Rendered summary of the flow result.
 */
std::string run_text_flow(
    const arcs::artifact::ArtifactVersion& input_artifact,
    const FlowOptions& options = {});

} // namespace arcs::core

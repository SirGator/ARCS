/**
 * @file common.hpp
 * @brief Small shared value types used across multiple core services.
 */

#pragma once

#include <string>

#include "artifact/artifact.hpp"

namespace arcs::core::services {

/**
 * @brief Flags extracted from parsing free-text input (demo-style yes/no
 *        answers and a policy-drift trigger).
 */
struct ParsedInput {
    bool approval_yes{false};
    bool permission_yes{false};
    bool policy_drift{false};
};

/**
 * @brief A policy artifact together with the previous version it replaced.
 */
struct PolicySnapshot {
    arcs::artifact::ArtifactVersion current;
    arcs::artifact::ArtifactVersion previous;
};

} // namespace arcs::core::services

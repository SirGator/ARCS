/**
 * @file policy.hpp
 * @brief Data model for policy artifacts governing capabilities, execution
 * constraints, verifier rules, and approval requirements.
 *
 * A policy payload defines what a subject is allowed to do (capabilities),
 * under what constraints (shell/net/file allow-lists), which checks a
 * verifier must run, and which capabilities require explicit approval.
 */
#pragma once

#include <string>
#include <vector>
#include <optional>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::policy {

using PolicyArtifact = arcs::artifact::ArtifactVersion;

/**
 * @brief Allow-list of shell commands permitted under a policy.
 */
struct ShellConstraints {
    std::vector<std::string> allow_cmd;
};

/**
 * @brief Allow-list of network domains permitted under a policy.
 */
struct NetConstraints {
    std::vector<std::string> allow_domains;
};

/**
 * @brief Allow-list of filesystem roots permitted under a policy.
 */
struct FileConstraints {
    std::vector<std::string> allow_roots;
};

/**
 * @brief Aggregates the optional shell, network, and file constraints that
 * a policy may impose. Each constraint category is absent if unrestricted
 * or not applicable.
 */
struct PolicyConstraints {
    std::optional<ShellConstraints> shell;
    std::optional<NetConstraints> net;
    std::optional<FileConstraints> file;
};

/**
 * @brief Sets of checks a verifier must run: hard checks that must pass and
 * soft checks whose failures are advisory.
 */
struct VerifierRules {
    std::vector<std::string> hard_checks;
    std::vector<std::string> soft_checks;
};

/**
 * @brief Full policy payload: granted capabilities, execution constraints,
 * verifier rules, and the subset of capabilities that require approval.
 */
struct PolicyPayload {
    std::vector<std::string> capabilities;
    PolicyConstraints constraints;
    VerifierRules verifier_rules;
    std::vector<std::string> approval_required_for;
};

} // namespace arcs::policy

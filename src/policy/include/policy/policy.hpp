#pragma once

#include <string>
#include <vector>
#include <optional>

namespace arcs::artifact {
struct ArtifactVersion;
}

namespace arcs::policy {

using PolicyArtifact = arcs::artifact::ArtifactVersion;

struct ShellConstraints {
    std::vector<std::string> allow_cmd;
};

struct NetConstraints {
    std::vector<std::string> allow_domains;
};

struct FileConstraints {
    std::vector<std::string> allow_roots;
};

struct PolicyConstraints {
    std::optional<ShellConstraints> shell;
    std::optional<NetConstraints> net;
    std::optional<FileConstraints> file;
};

struct VerifierRules {
    std::vector<std::string> hard_checks;
    std::vector<std::string> soft_checks;
};

struct PolicyPayload {
    std::vector<std::string> capabilities;
    PolicyConstraints constraints;
    VerifierRules verifier_rules;
    std::vector<std::string> approval_required_for;
};

} // namespace arcs::policy

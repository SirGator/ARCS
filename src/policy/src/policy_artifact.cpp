#include "policy/policy.hpp"

#include <nlohmann/json.hpp>

namespace arcs::policy {

namespace {

std::vector<std::string> string_vec_from_json(
    const nlohmann::json& j,
    const char* key
) {
    std::vector<std::string> out;

    if (!j.contains(key)) {
        return out;
    }

    for (const auto& item : j.at(key)) {
        out.push_back(item.get<std::string>());
    }

    return out;
}

} // namespace

PolicyPayload policy_from_json(const nlohmann::json& j) {
    PolicyPayload policy{};

    policy.capabilities =
        string_vec_from_json(j, "capabilities");

    policy.approval_required_for =
        string_vec_from_json(j, "approval_required_for");

    if (j.contains("verifier_rules")) {
        const auto& vr = j.at("verifier_rules");

        policy.verifier_rules.hard_checks =
            string_vec_from_json(vr, "hard_checks");

        policy.verifier_rules.soft_checks =
            string_vec_from_json(vr, "soft_checks");
    }

    if (j.contains("constraints")) {
        const auto& c = j.at("constraints");

        if (c.contains("shell")) {
            ShellConstraints shell{};
            shell.allow_cmd =
                string_vec_from_json(c.at("shell"), "allow_cmd");
            policy.constraints.shell = shell;
        }

        if (c.contains("net")) {
            NetConstraints net{};
            net.allow_domains =
                string_vec_from_json(c.at("net"), "allow_domains");
            policy.constraints.net = net;
        }

        if (c.contains("file")) {
            FileConstraints file{};
            file.allow_roots =
                string_vec_from_json(c.at("file"), "allow_roots");
            policy.constraints.file = file;
        }
    }

    return policy;
}

nlohmann::json policy_to_json(const PolicyPayload& policy) {
    nlohmann::json j;

    j["capabilities"] = policy.capabilities;
    j["approval_required_for"] = policy.approval_required_for;

    j["verifier_rules"] = {
        {"hard_checks", policy.verifier_rules.hard_checks},
        {"soft_checks", policy.verifier_rules.soft_checks}
    };

    j["constraints"] = nlohmann::json::object();

    if (policy.constraints.shell.has_value()) {
        j["constraints"]["shell"] = {
            {"allow_cmd", policy.constraints.shell->allow_cmd}
        };
    }

    if (policy.constraints.net.has_value()) {
        j["constraints"]["net"] = {
            {"allow_domains", policy.constraints.net->allow_domains}
        };
    }

    if (policy.constraints.file.has_value()) {
        j["constraints"]["file"] = {
            {"allow_roots", policy.constraints.file->allow_roots}
        };
    }

    return j;
}

} // namespace arcs::policy
/**
 * @file policy_artifact.cpp
 * @brief Implements JSON (de)serialization for PolicyPayload, declared in
 * policy/policy.hpp.
 */
#include "policy/policy.hpp"

#include <nlohmann/json.hpp>

namespace arcs::policy {

namespace {

/**
 * @brief Reads a JSON array field into a vector of strings, or returns an
 * empty vector if the field is absent.
 * @param j JSON object to read from.
 * @param key Name of the array field to extract.
 * @return Vector of strings from the field, empty if the field is missing.
 */
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

/**
 * @brief Parses a full policy payload from its JSON representation,
 * including capabilities, constraints, verifier rules, and approval
 * requirements. Missing sections default to empty.
 * @param j JSON object representing a policy.
 * @return The parsed PolicyPayload.
 */
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

/**
 * @brief Serializes a policy payload to its JSON representation, emitting
 * only the constraint sub-objects that are actually present.
 * @param policy The policy payload to serialize.
 * @return JSON object representing the policy.
 */
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
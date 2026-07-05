/**
 * @file action_materializer.cpp
 * @brief Implements ActionMaterializer: validates that an option artifact
 *        is correctly bound to a policy artifact, extracts the option's
 *        steps, checks each step against the policy's allowed
 *        capabilities, and turns each into a deterministic action
 *        artifact ready for execution.
 */
#include <nlohmann/json.hpp>
#include <cstdint>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "materializer.hpp"
#include "step.hpp"
#include "artifact/artifact.hpp"
#include "artifact/factory.hpp"
#include "policy/policy.hpp"

namespace arcs::execution {

namespace {

/**
 * @brief Check whether a JSON array value contains a given string.
 * @param value JSON value expected to be an array (returns false otherwise).
 * @param needle String to search for among the array's string elements.
 * @return True if @p value is an array containing @p needle.
 */
bool json_array_contains_string(const nlohmann::json& value, const std::string& needle)
{
    if (!value.is_array()) {
        return false;
    }

    for (const auto& entry : value) {
        if (entry.is_string() && entry.get<std::string>() == needle) {
            return true;
        }
    }

    return false;
}

/**
 * @brief Compute the FNV-1a hash of a string and render it as hex.
 * @param value Bytes to hash.
 * @return Lowercase hexadecimal representation of the 64-bit FNV-1a hash.
 */
std::string fnv1a_hex(std::string_view value)
{
    std::uint64_t hash = 14695981039346656037ull;
    for (const unsigned char ch : value) {
        hash ^= ch;
        hash *= 1099511628211ull;
    }

    std::ostringstream out;
    out << std::hex << hash;
    return out.str();
}

/**
 * @brief Derive a stable action id for a step from a fingerprint of the
 *        option, policy, and step contents, so repeated materialization
 *        of the same inputs yields the same action id.
 * @param step Step being materialized into an action.
 * @param option Option artifact the step originates from.
 * @param policy Policy artifact bound to the option.
 * @return A "act_"-prefixed deterministic action id.
 */
std::string deterministic_action_key(const Step& step,
                                     const OptionArtifact& option,
                                     const PolicyArtifact& policy)
{
    nlohmann::json fingerprint = {
        {"option_artifact_id", option.artifact_id},
        {"option_version_id", option.version_id},
        {"stream_key", option.stream_key},
        {"policy_artifact_id", policy.artifact_id},
        {"policy_version_id", policy.version_id},
    };

    if (std::holds_alternative<EmitReportStep>(step)) {
        const auto& emit_report = std::get<EmitReportStep>(step);
        fingerprint["step"] = {
            {"kind", "emit_report"},
            {"format", emit_report.params.format},
            {"sections", emit_report.params.sections},
        };
    }

    return std::string{"act_"} + fnv1a_hex(fingerprint.dump());
}

/**
 * @brief Build the artifact id for an action artifact from its action id.
 * @param action_id Deterministic action id (see deterministic_action_key).
 * @return An "a_"-prefixed artifact id.
 */
std::string deterministic_action_artifact_id(const std::string& action_id)
{
    return "a_" + action_id;
}

/**
 * @brief Build the version id for an action artifact, tying it to the
 *        policy version it was materialized under.
 * @param action_id Deterministic action id (see deterministic_action_key).
 * @param policy Policy artifact the action was materialized against.
 * @return A "v_"-prefixed version id combining the action id and policy version.
 */
std::string deterministic_action_version_id(const std::string& action_id,
                                            const PolicyArtifact& policy)
{
    return "v_" + action_id + "_" + policy.version_id;
}

/**
 * @brief Verify that an option artifact declares a policy_ref matching
 *        the given policy artifact, throwing on any mismatch or malformed
 *        data.
 * @param option Option artifact expected to reference @p policy.
 * @param policy Policy artifact the option should be bound to.
 * @throws std::runtime_error if policy_ref is missing, malformed, or
 *         does not match @p policy (policy drift).
 */
void validate_policy_binding(const OptionArtifact& option, const PolicyArtifact& policy)
{
    if (!option.payload.is_object() || !option.payload.contains("policy_ref")) {
        throw std::runtime_error("option.policy_ref missing");
    }

    const auto& policy_ref = option.payload.at("policy_ref");
    if (!policy_ref.is_object() ||
        !policy_ref.contains("artifact_id") ||
        !policy_ref.contains("version_id")) {
        throw std::runtime_error("option.policy_ref malformed");
    }

    const auto option_policy_artifact_id = policy_ref.at("artifact_id").get<std::string>();
    const auto option_policy_version_id = policy_ref.at("version_id").get<std::string>();

    if (option_policy_artifact_id != policy.artifact_id ||
        option_policy_version_id != policy.version_id) {
        throw std::runtime_error("policy drift detected");
    }
}

/**
 * @brief Verify that a policy artifact grants the "exec:report_emit"
 *        capability.
 * @param policy Policy artifact to check.
 * @throws std::runtime_error if capabilities are missing or do not
 *         include "exec:report_emit".
 */
void validate_policy_allows_report_emit(const PolicyArtifact& policy)
{
    if (!policy.payload.is_object() || !policy.payload.contains("capabilities")) {
        throw std::runtime_error("policy.capabilities missing");
    }

    const auto& capabilities = policy.payload.at("capabilities");
    if (!json_array_contains_string(capabilities, "exec:report_emit")) {
        throw std::runtime_error("policy does not allow exec:report_emit");
    }
}

/**
 * @brief Parse the "steps" array out of an option artifact's payload into
 *        typed Step values. Currently only "emit_report" steps are
 *        supported; any other kind is rejected.
 * @param option Option artifact whose payload may contain a "steps" array.
 * @return The parsed steps, or an empty vector if the option has no steps.
 * @throws std::runtime_error if steps is present but not an array, if a
 *         step is missing its "kind", or if a step's kind is unsupported.
 */
std::vector<Step> extract_steps(const OptionArtifact& option)
{
    std::vector<Step> steps;

    if (!option.payload.is_object() || !option.payload.contains("steps")) {
        return steps;
    }

    const auto& raw_steps = option.payload.at("steps");
    if (!raw_steps.is_array()) {
        throw std::runtime_error("option.steps must be an array");
    }

    for (const auto& raw_step : raw_steps) {
        if (!raw_step.is_object() || !raw_step.contains("kind")) {
            throw std::runtime_error("step.kind missing");
        }

        const auto kind = raw_step.at("kind").get<std::string>();
        if (kind == "emit_report") {
            EmitReportStep step{};
            if (raw_step.contains("params")) {
                const auto& params = raw_step.at("params");
                step.params.format = params.value("format", std::string{});
                if (params.contains("sections") && params.at("sections").is_array()) {
                    for (const auto& section : params.at("sections")) {
                        if (section.is_string()) {
                            step.params.sections.push_back(section.get<std::string>());
                        }
                    }
                }
            }
            steps.push_back(step);
            continue;
        }

        throw std::runtime_error("unsupported step.kind");
    }

    return steps;
}

} // namespace

namespace {

/**
 * @brief Validate that a step is of a supported kind and permitted by the
 *        given policy.
 * @param step Step to validate.
 * @param policy Policy artifact the step must be permitted under.
 * @throws std::runtime_error if the step kind is unsupported or the
 *         policy does not grant the required capability.
 */
void validate_step_against_policy(const Step& step, const PolicyArtifact& policy)
{
    if (!std::holds_alternative<EmitReportStep>(step)) {
        throw std::runtime_error("unsupported step.kind");
    }

    validate_policy_allows_report_emit(policy);
}

/**
 * @brief Convert a validated step into a fully-populated action-candidate
 *        artifact, filling in its deterministic ids, payload, and
 *        provenance.
 * @param step Step to convert; currently only EmitReportStep is supported.
 * @param option Option artifact the step originates from.
 * @param policy Policy artifact the option is bound to.
 * @return The resulting action-candidate artifact.
 * @throws std::runtime_error if the step kind is unsupported.
 */
ActionArtifact step_to_action(const Step& step,
                              const OptionArtifact& option,
                              const PolicyArtifact& policy)
{
    (void)policy;

    if (std::holds_alternative<EmitReportStep>(step)) {
        const auto& s = std::get<EmitReportStep>(step);

        ActionArtifact action = arcs::artifact::factory::make_base_artifact(
            "action_candidate",
            "arcs.action_candidate.report_emit.v1",
            option.stream_key,
            "system",
            "action_materializer",
            "internal",
            "option_to_action_candidate",
            "high",
            "system");

        const auto action_id = deterministic_action_key(step, option, policy);
        auto required_permissions = nlohmann::json::array();
        required_permissions.push_back("exec:report_emit");
        action.artifact_id = deterministic_action_artifact_id(action_id);
        action.version_id = deterministic_action_version_id(action_id, policy);

        action.payload = nlohmann::json{
            {"action_id", action_id},
            {"type", "report_emit"},
            {"option_ref", {{"artifact_id", option.artifact_id}, {"version_id", option.version_id}}},
            {"policy_ref", {{"artifact_id", policy.artifact_id}, {"version_id", policy.version_id}}},
            {"params", {
                {"format", s.params.format},
                {"sections", s.params.sections}
            }},
            {"required_permissions", required_permissions},
            {"safety_level", "low"},
            {"idempotency_key", action_id}
        };
        action.provenance.parents = {option.artifact_id};
        action.provenance.rules_applied = {"materialize_emit_report"};
        action.provenance.transform = "materialize_action_candidate";

        return action;
    }

    throw std::runtime_error("unsupported step.kind");
}

} // namespace

/**
 * @brief Materialize an option's steps into action artifacts: validates
 *        the option-policy binding, extracts the steps, validates each
 *        against the policy, and converts each into an action artifact.
 * @param option Option artifact whose steps are to be materialized.
 * @param policy Policy artifact the option must be bound to and that
 *               constrains which actions are permitted.
 * @return The list of action artifacts derived from the option.
 * @throws std::runtime_error on policy binding/capability violations or
 *         malformed/unsupported steps.
 */
std::vector<ActionArtifact> ActionMaterializer::materialize(
    const OptionArtifact& option,
    const PolicyArtifact& policy) const
{
    std::vector<ActionArtifact> actions;

    validate_policy_binding(option, policy);

    const auto steps = extract_steps(option);

    for (const auto& step : steps) {
        validate_step_against_policy(step, policy);
        actions.push_back(step_to_action(step, option, policy));
    }

    return actions;
}

} // namespace arcs::execution

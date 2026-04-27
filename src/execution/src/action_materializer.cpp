#include <nlohmann/json.hpp>
#include <stdexcept>
#include <string>
#include <vector>

#include "materializer.hpp"
#include "step.hpp"
#include "artifact/artifact.hpp"
#include "artifact/factory.hpp"
#include "artifact/ids.hpp"
#include "policy/policy.hpp"

namespace arcs::execution {

namespace {

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

void validate_step_against_policy(const Step& step, const PolicyArtifact& policy)
{
    if (!std::holds_alternative<EmitReportStep>(step)) {
        throw std::runtime_error("unsupported step.kind");
    }

    validate_policy_allows_report_emit(policy);
}

ActionArtifact step_to_action(const Step& step,
                              const OptionArtifact& option,
                              const PolicyArtifact& policy)
{
    (void)policy;

    if (std::holds_alternative<EmitReportStep>(step)) {
        const auto& s = std::get<EmitReportStep>(step);

        ActionArtifact action = arcs::artifact::factory::make_base_artifact(
            "action",
            "arcs.action.v1",
            option.stream_key,
            "system",
            "action_materializer",
            "internal",
            "option_to_action",
            "high",
            "system");

        const auto action_id = arcs::artifact::ids::new_event_id();
        auto required_permissions = nlohmann::json::array();
        required_permissions.push_back("exec:report_emit");

        action.payload = nlohmann::json{
            {"action_id", action_id},
            {"schema_id", "actions/report_emit@v1"},
            {"type", "report_emit"},
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
        action.provenance.transform = "materialize_action";

        return action;
    }

    throw std::runtime_error("unsupported step.kind");
}

} // namespace

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

#include <stdexcept>
#include <string>
#include <vector>

#include "materializer.hpp"
#include "step.hpp"
#include "artifact/artifact.hpp"
#include "policy/policy.hpp"
#include "artifact/ids.hpp"

namespace arcs::execution {

namespace {

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
    (void)step;
    (void)policy;
}

ActionArtifact step_to_action(const Step& step,
                              const OptionArtifact& option,
                              const PolicyArtifact& policy)
{
    (void)policy;

    if (std::holds_alternative<EmitReportStep>(step)) {
        const auto& s = std::get<EmitReportStep>(step);

        ActionArtifact action{};
        action.artifact_id = arcs::artifact::ids::new_artifact_id();
        action.version_id = arcs::artifact::ids::new_version_id();
        action.version = 1;
        action.type = "action";
        action.schema_id = "arcs.action.v1";
        action.schema_version = 1;
        action.stream_key = option.stream_key;

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

    const auto steps = extract_steps(option);

    for (const auto& step : steps) {
        validate_step_against_policy(step, policy);
        actions.push_back(step_to_action(step, option, policy));
    }

    return actions;
}

} // namespace arcs::execution

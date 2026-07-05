#include "core/runtime/runtime_support.hpp"

#include <filesystem>

#include "core/stages/action_stage.hpp"
#include "core/stages/approval_stage.hpp"
#include "core/stages/execution_stage.hpp"
#include "core/stages/ingress_stage.hpp"
#include "core/stages/interpretation_stage.hpp"
#include "core/stages/planning_stage.hpp"
#include "core/stages/task_stage.hpp"
#include "core/stages/verification_stage.hpp"
#include "schema/schema_loader.hpp"

namespace arcs::core::runtime {
namespace {

void push_if_missing(
    std::vector<arcs::artifact::ArtifactVersion>& artifacts,
    const arcs::store::IStore& store,
    const arcs::artifact::ArtifactVersion& artifact)
{
    if (artifact.artifact_id.empty() || !store.has_artifact(artifact.artifact_id)) {
        artifacts.push_back(artifact);
    }
}

} // namespace

const arcs::schema::SchemaRegistry& default_payload_schema_registry()
{
    static const auto registry = [] {
        arcs::schema::SchemaRegistry registry;
        const auto schemas_dir = std::filesystem::path(__FILE__).parent_path()
            .parent_path().parent_path().parent_path().parent_path()
            / "schemas" / "v1";

        for (const auto& entry : std::filesystem::directory_iterator(schemas_dir)) {
            if (!entry.is_regular_file() || entry.path().extension() != ".json") {
                continue;
            }

            const auto schema_entry = arcs::schema::SchemaLoader::load_from_file(entry.path());
            if (!schema_entry.has_value() || !registry.register_schema(*schema_entry)) {
                throw std::runtime_error("payload schema registry could not be loaded");
            }
        }

        return registry;
    }();

    return registry;
}

FlowResult finalize_runtime(
    RuntimeContext& context)
{
    if (context.flow.status == FlowStatus::Blocked && context.flow.reason.empty()) {
        context.flow.reason = "missing approval or permission";
    }

    if (!context.flow.option.artifact_id.empty()) {
        auto decision_artifact = context.dependencies.decision_service.make_decision(
            context.flow.option,
            context.flow.report,
            context.flow.status == FlowStatus::Completed ? "not_blocked" : "blocked",
            context.flow.reason,
            context.flow.approval_artifact.artifact_id,
            context.flow.action_artifact.artifact_id,
            context.flow.execution_result_artifact.artifact_id);

        if (!context.flow.action_artifact.artifact_id.empty()) {
            std::vector<arcs::artifact::ArtifactVersion> artifacts;
            if (!context.flow.execution_result_artifact.artifact_id.empty()) {
                artifacts.push_back(context.flow.execution_result_artifact);
            }
            artifacts.push_back(decision_artifact);
            context.dependencies.commit_service.commit_and_collect(
                context.dependencies.store,
                context.flow.persisted_bundle,
                "decision.recorded",
                artifacts,
                utc_now());
        } else {
            std::vector<arcs::artifact::ArtifactVersion> artifacts;
            if (context.flow.persisted_bundle.versions.empty()) {
                artifacts.push_back(context.flow.ingress_event);
                if (context.flow.interpretation_artifact.has_value()) {
                    artifacts.push_back(*context.flow.interpretation_artifact);
                }
                if (context.flow.interpretation_report_artifact.has_value()) {
                    artifacts.push_back(*context.flow.interpretation_report_artifact);
                }
                artifacts.push_back(context.flow.task_artifact);
                artifacts.push_back(context.flow.policy_previous);
                artifacts.push_back(context.flow.policy_current);
                if (context.flow.permission_resolution.has_value()) {
                    for (const auto& permission_artifact : context.flow.permission_resolution->artifacts) {
                        artifacts.push_back(permission_artifact);
                    }
                }
                artifacts.push_back(context.flow.option);
                artifacts.push_back(context.flow.report_artifact);
                if (!context.flow.approval_request_artifact.artifact_id.empty()) {
                    artifacts.push_back(context.flow.approval_request_artifact);
                }
                if (!context.flow.action_candidate_artifact.artifact_id.empty()) {
                    artifacts.push_back(context.flow.action_candidate_artifact);
                }
                if (!context.flow.approval_artifact.artifact_id.empty()) {
                    artifacts.push_back(context.flow.approval_artifact);
                }
                if (!context.flow.action_report_artifact.artifact_id.empty()) {
                    artifacts.push_back(context.flow.action_report_artifact);
                }
            }
            artifacts.push_back(decision_artifact);
            context.dependencies.commit_service.commit_and_collect(
                context.dependencies.store,
                context.flow.persisted_bundle,
                "decision.recorded",
                artifacts,
                utc_now());
        }
    }

    return finalize_flow_result(context, std::string{});
}

FlowResult run_fresh_flow(
    RuntimeContext& context)
{
    const stages::IngressStage ingress_stage;
    const stages::InterpretationStage interpretation_stage;
    const stages::TaskStage task_stage;
    const stages::PlanningStage planning_stage;
    const stages::VerificationStage verification_stage;
    const stages::ActionStage action_stage;
    const stages::ApprovalStage approval_stage;
    const stages::ExecutionStage execution_stage;

    if (!ingress_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!interpretation_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!task_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!planning_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!verification_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!action_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!approval_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }
    if (!execution_stage.run(context).continue_flow()) {
        return finalize_runtime(context);
    }

    return finalize_runtime(context);
}

FlowResult resume_approval_flow(
    RuntimeContext& context,
    const arcs::artifact::ArtifactVersion& input_artifact)
{
    const auto resume_state = context.dependencies.resume_service.resume_from_approval(input_artifact, context.dependencies.store);
    if (!resume_state.ok) {
        context.logger.fail("resume approval", resume_state.error);
        context.flow.reason = resume_state.error;
        return finalize_runtime(context);
    }
    context.flow.option = resume_state.option;
    context.flow.policy_current = resume_state.policy;
    context.flow.action_candidate_artifact = resume_state.action_candidate;
    context.flow.report = resume_state.option_report;
    context.flow.report_artifact = context.dependencies.verification_service.make_named_report_artifact(
        context.flow.option,
        context.flow.report,
        "a_resume_option_verification_report",
        "v_resume_option_verification_report");

    context.logger.ok("resume approval", "approval artifact accepted | approval_id=" + input_artifact.artifact_id);

    context.flow.permission_time_source.emplace(utc_now());
    context.flow.permission_resolution = context.dependencies.permission_service.resolve_permissions(
        "user:cli",
        context.flow.option.stream_key,
        *context.flow.permission_time_source,
        context.dependencies.permission_source);

    std::vector<arcs::artifact::ArtifactVersion> resume_artifacts{
        input_artifact,
        context.flow.report_artifact,
    };
    push_if_missing(resume_artifacts, context.dependencies.store, context.flow.option);
    push_if_missing(resume_artifacts, context.dependencies.store, context.flow.policy_current);
    push_if_missing(resume_artifacts, context.dependencies.store, context.flow.action_candidate_artifact);
    for (const auto& permission_artifact : context.flow.permission_resolution->artifacts) {
        push_if_missing(resume_artifacts, context.dependencies.store, permission_artifact);
    }
    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "approval.resumed",
        resume_artifacts,
        utc_now());

    context.flow.action_artifact = context.dependencies.action_service.promote_candidate(
        context.flow.action_candidate_artifact,
        input_artifact,
        context.dependencies.approval_service);
    context.logger.ok(
        "promote action",
        "approved action created | action_id=" + context.flow.action_artifact.artifact_id +
        " candidate_ref=" + context.flow.action_candidate_artifact.artifact_id + ":" + context.flow.action_candidate_artifact.version_id);

    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "action.promoted",
        {context.flow.action_artifact},
        utc_now());

    const auto action_verification_context = context.dependencies.action_service.build_verification_context(
        context.flow.policy_current,
        context.flow.permission_resolution->permissions,
        context.dependencies.schema_registry,
        context.dependencies.store,
        *context.flow.permission_time_source);

    const auto action_report = context.dependencies.verification_service.verify_action(
        context.flow.action_artifact,
        action_verification_context);
    context.flow.action_report_artifact = context.dependencies.verification_service.make_named_report_artifact(
        context.flow.action_artifact,
        action_report,
        "a_resume_action_verification_report",
        "v_resume_action_verification_report");
    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "action.verified",
        {context.flow.action_report_artifact},
        utc_now());

    if (action_report.status != arcs::verification::CheckStatus::Pass) {
        context.logger.fail("action_verification_report", arcs::verification::to_string(action_report.status));
        context.flow.reason = first_blocker_or(action_report, "action verification blocked");
        return finalize_runtime(context);
    }

    context.logger.ok(
        "action_verification_report",
        "pass | checks=" + std::to_string(action_report.checks.size()));

    arcs::execution::ExecutionContext execution_context{};
    execution_context.approval_id = input_artifact.artifact_id;
    execution_context.verification_id = context.flow.action_report_artifact.artifact_id;
    execution_context.approval_valid = true;
    execution_context.approval_expires_at = input_artifact.payload.value("expires_at", std::string{});
    execution_context.verification_passed = true;
    execution_context.granted_permissions = context.flow.permission_resolution->permissions.capabilities;

    const auto execution_outcome = context.dependencies.execution_service.execute_report_action(
        context.flow.action_artifact,
        execution_context);
    if (execution_outcome.result.status != arcs::execution::ExecutionStatus::Success) {
        context.logger.fail(
            "decision",
            execution_outcome.result.error_message.empty()
                ? "execution blocked"
                : execution_outcome.result.error_message);
        context.flow.reason = execution_outcome.result.error_message.empty()
            ? "execution blocked"
            : execution_outcome.result.error_message;
        return finalize_runtime(context);
    }

    context.logger.ok(
        "execute action",
        "report_emit success | action_id=" + context.flow.action_artifact.artifact_id +
        " exit_code=" + std::to_string(execution_outcome.result.exit_code));
    context.flow.execution_result_artifact = *execution_outcome.result_artifact;
    context.flow.status = FlowStatus::Completed;
    context.flow.reason = "approval resumed and action executed";
    context.logger.ok("decision", "not_blocked");

    return finalize_runtime(context);
}

} // namespace arcs::core::runtime

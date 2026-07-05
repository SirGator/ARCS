/**
 * @file action_stage.cpp
 * @brief Implements the pipeline stage that materializes an action candidate
 *        from a verified report option, creates the corresponding approval
 *        request artifact, commits the pre-approval artifact bundle, and
 *        halts the flow (as Pending) until an approval has been supplied.
 */

#include "core/stages/action_stage.hpp"

#include "materializer.hpp"

namespace arcs::core::stages {

/**
 * @brief Materializes an action candidate for the current option, builds and
 *        commits an approval request together with all artifacts that
 *        preceded approval, and blocks the flow until approval is granted.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the action candidate, approval request, and pending state.
 * @return StageResult::Blocked if no action could be materialized,
 *         StageResult::Pending if approval has not yet been granted, or
 *         StageResult::Continue once the approval request has been recorded
 *         and approval is already marked as granted.
 */
StageResult ActionStage::run(runtime::RuntimeContext& context) const
{
    const auto candidate = context.dependencies.action_service.materialize_candidate(context.flow.option, context.flow.policy_current);
    if (!candidate.has_value()) {
        context.flow.reason = "no action materialized";
        context.logger.fail("decision", context.flow.reason);
        return {.status = StageStatus::Blocked, .reason = context.flow.reason};
    }

    context.flow.action_candidate_artifact = *candidate;
    const auto risk_summary = context.dependencies.action_service.risk_summary(context.flow.option, context.flow.action_candidate_artifact);
    context.logger.ok(
        "materialize action_candidate",
        "report_emit | action_candidate_id=" + context.flow.action_candidate_artifact.artifact_id +
        " version=" + context.flow.action_candidate_artifact.version_id);

    context.flow.approval_request_artifact = context.dependencies.approval_service.create_approval_request(
        context.flow.option,
        context.flow.policy_current,
        context.flow.report_artifact,
        context.flow.action_candidate_artifact,
        runtime::utc_now(),
        context.flow.report_artifact.version_id,
        risk_summary);
    context.logger.ok(
        "approval_request",
        "artifact created | request_id=" + context.flow.approval_request_artifact.artifact_id +
        " action_candidate_ref=" + context.flow.action_candidate_artifact.artifact_id + ":" + context.flow.action_candidate_artifact.version_id);

    std::vector<arcs::artifact::ArtifactVersion> pre_approval_artifacts{
        context.flow.ingress_event,
    };
    if (context.flow.interpretation_artifact.has_value()) {
        pre_approval_artifacts.push_back(*context.flow.interpretation_artifact);
    }
    if (context.flow.interpretation_report_artifact.has_value()) {
        pre_approval_artifacts.push_back(*context.flow.interpretation_report_artifact);
    }
    pre_approval_artifacts.push_back(context.flow.task_artifact);
    pre_approval_artifacts.push_back(context.flow.policy_previous);
    pre_approval_artifacts.push_back(context.flow.policy_current);
    for (const auto& permission_artifact : context.flow.permission_resolution->artifacts) {
        pre_approval_artifacts.push_back(permission_artifact);
    }
    pre_approval_artifacts.push_back(context.flow.option);
    pre_approval_artifacts.push_back(context.flow.report_artifact);
    pre_approval_artifacts.push_back(context.flow.approval_request_artifact);
    pre_approval_artifacts.push_back(context.flow.action_candidate_artifact);
    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "approval.requested",
        pre_approval_artifacts,
        runtime::utc_now());

    if (!context.flow.parsed_input.approval_yes) {
        context.flow.status = FlowStatus::Pending;
        context.flow.reason = "approval pending";
        context.flow.pending_state = PendingState{
            .kind = "approval_request",
            .artifact_id = context.flow.approval_request_artifact.artifact_id,
        };
        context.messages.push_back(
            "resume: submit an approval artifact bound to approval_request " +
            context.flow.approval_request_artifact.artifact_id + "\n");
        context.logger.fail("approval", "approval missing or not yes");
        return {.status = StageStatus::Pending, .reason = context.flow.reason};
    }

    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

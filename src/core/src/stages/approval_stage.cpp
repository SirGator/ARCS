/**
 * @file approval_stage.cpp
 * @brief Implements the pipeline stage that records an approval for the
 *        pending action candidate, promotes the candidate into an approved
 *        action, runs verification on that action, and commits each
 *        resulting artifact to the store.
 */

#include "core/stages/approval_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Submits a (demo) approval for the current action candidate, commits
 *        it, promotes the candidate to an approved action, verifies that
 *        action, and commits the verification report.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the approval artifact, promoted action artifact, and action
 *        verification report.
 * @return StageResult::Blocked if action verification does not pass,
 *         otherwise StageResult::Continue.
 */
StageResult ApprovalStage::run(runtime::RuntimeContext& context) const
{
    const auto risk_summary = context.dependencies.action_service.risk_summary(context.flow.option, context.flow.action_candidate_artifact);
    const auto store_head_at_approval = context.dependencies.store.current_head_version_id(context.flow.action_candidate_artifact.artifact_id)
        .value_or(context.flow.action_candidate_artifact.version_id);

    context.flow.approval_artifact = context.dependencies.approval_service.submit_demo_approval(
        context.flow.option,
        context.flow.policy_current,
        context.flow.report_artifact,
        context.flow.approval_request_artifact,
        context.flow.action_candidate_artifact,
        runtime::utc_now(),
        runtime::utc_after_hours(1),
        store_head_at_approval,
        risk_summary);

    context.logger.ok(
        "approval",
        "approval artifact created | approval_id=" + context.flow.approval_artifact.artifact_id +
        " request_ref=" + context.flow.approval_request_artifact.artifact_id +
        " action_candidate_ref=" + context.flow.action_candidate_artifact.artifact_id + ":" + context.flow.action_candidate_artifact.version_id);

    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "approval.granted",
        {context.flow.approval_artifact},
        runtime::utc_now());

    context.flow.action_artifact = context.dependencies.action_service.promote_candidate(
        context.flow.action_candidate_artifact,
        context.flow.approval_artifact,
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
        runtime::utc_now());

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
        "a_action_verification_report",
        "v_action_verification_report");

    if (action_report.status == arcs::verification::CheckStatus::Pass) {
        context.logger.ok(
            "action_verification_report",
            "pass | checks=" + std::to_string(action_report.checks.size()));
    } else {
        context.logger.fail(
            "action_verification_report",
            arcs::verification::to_string(action_report.status));
    }

    context.dependencies.commit_service.commit_and_collect(
        context.dependencies.store,
        context.flow.persisted_bundle,
        "action.verified",
        {context.flow.action_report_artifact},
        runtime::utc_now());

    if (action_report.status != arcs::verification::CheckStatus::Pass) {
        context.flow.reason = runtime::first_blocker_or(action_report, "action verification blocked");
        context.logger.fail("decision", context.flow.reason);
        return {.status = StageStatus::Blocked, .reason = context.flow.reason};
    }

    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

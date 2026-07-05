/**
 * @file planning_stage.cpp
 * @brief Implements the pipeline stage that loads the current and previous
 *        policy snapshots for the task's stream, builds a report option
 *        bound to the appropriate policy reference (honoring simulated
 *        policy drift), and logs the approval/permission/policy-drift
 *        preconditions for that option.
 */

#include "core/stages/planning_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Loads the policy snapshot for the task's stream, creates a report
 *        option bound to either the current or previous policy (depending on
 *        the parsed policy-drift flag), and logs the approval, permission,
 *        and policy-drift check outcomes.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the current/previous policy artifacts and the new option
 *        artifact.
 * @return StageResult::Continue; this stage does not block the flow.
 */
StageResult PlanningStage::run(runtime::RuntimeContext& context) const
{
    const auto policy_snapshot = context.dependencies.policy_service.load_policy_snapshot(context.flow.task_artifact.stream_key);
    context.flow.policy_current = policy_snapshot.current;
    context.flow.policy_previous = context.dependencies.policy_service.previous_policy_for_scope(context.flow.task_artifact.stream_key)
        .value_or(policy_snapshot.previous);

    const auto policy_ref = context.flow.parsed_input.policy_drift ? context.flow.policy_previous : context.flow.policy_current;
    context.flow.option = context.dependencies.planning_service.create_report_option(context.flow.task_artifact, policy_ref, context.input);

    context.logger.ok(
        "option",
        "artifact created | policy_ref=" + policy_ref.artifact_id + ":" + policy_ref.version_id + " action=report_emit");

    if (context.flow.parsed_input.approval_yes) {
        context.logger.ok("check approval", "approval=yes");
    } else {
        context.logger.fail("check approval", "approval missing or not yes");
    }

    if (context.flow.parsed_input.permission_yes) {
        context.logger.ok("check permission", "permission=yes");
    } else {
        context.logger.fail("check permission", "permission missing or not yes");
    }

    if (context.flow.parsed_input.policy_drift) {
        context.logger.fail("policy drift", "option bound to stale policy ref");
    } else {
        context.logger.ok("policy drift", "option policy binding matches current head");
    }

    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

/**
 * @file execution_stage.cpp
 * @brief Implements the final pipeline stage that executes the approved and
 *        verified action, recording the outcome as either a completed
 *        (success) or failed (blocked/error) result.
 */

#include "core/stages/execution_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Builds an execution context from the approval and verification
 *        results already recorded on the pipeline context, then executes the
 *        approved action.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the execution result artifact, decision status, and reason.
 * @return StageResult::Completed if execution succeeds, otherwise
 *         StageResult::Failed with the execution error message (or a default
 *         "execution blocked" reason).
 */
StageResult ExecutionStage::run(runtime::RuntimeContext& context) const
{
    arcs::execution::ExecutionContext execution_context{};
    execution_context.approval_id = context.flow.approval_artifact.artifact_id;
    execution_context.verification_id = context.flow.action_report_artifact.artifact_id;
    execution_context.approval_valid = true;
    execution_context.approval_expires_at = context.flow.approval_artifact.payload.value("expires_at", std::string{});
    execution_context.verification_passed = true;
    execution_context.granted_permissions = context.flow.permission_resolution->permissions.capabilities;

    const auto execution_outcome = context.dependencies.execution_service.execute_report_action(
        context.flow.action_artifact,
        execution_context);
    const auto& execution_result = execution_outcome.result;

    if (execution_result.status == arcs::execution::ExecutionStatus::Success) {
        context.logger.ok(
            "execute action",
            "report_emit success | action_id=" + context.flow.action_artifact.artifact_id +
            " exit_code=" + std::to_string(execution_result.exit_code));
        context.flow.execution_result_artifact = *execution_outcome.result_artifact;
        context.flow.status = FlowStatus::Completed;
        context.flow.reason = context.options.enable_demo_controls
            ? "demo approval and permission granted"
            : "approved action executed";
        context.logger.ok("decision", "not_blocked");
        return {.status = StageStatus::Completed};
    }

    context.flow.status = FlowStatus::Failed;
    context.flow.reason = execution_result.error_message.empty() ? "execution blocked" : execution_result.error_message;
    context.logger.fail("decision", context.flow.reason);
    return {.status = StageStatus::Failed, .reason = context.flow.reason};
}

} // namespace arcs::core::stages

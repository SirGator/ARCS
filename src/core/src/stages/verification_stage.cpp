/**
 * @file verification_stage.cpp
 * @brief Implements the pipeline stage that resolves permissions, builds a
 *        policy-driven verification plan, runs all applicable verifiers
 *        against the current report option, and blocks the flow if
 *        verification does not pass.
 */

#include "core/stages/verification_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Resolves permissions for the current user/stream, builds a
 *        verification engine from the policy's verification plan, runs it
 *        against the report option, appends any unsupported checks, and
 *        produces the resulting verification report artifact.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the permission resolution, verification report, and report
 *        artifact.
 * @return StageResult::Blocked if verification does not pass, otherwise
 *         StageResult::Continue.
 */
StageResult VerificationStage::run(runtime::RuntimeContext& context) const
{
    arcs::verification::VerificationEngine verification_engine;
    const auto verification_plan = context.dependencies.verification_service.build_policy_plan(context.flow.policy_current);
    for (const auto& verifier : verification_plan.verifiers) {
        verification_engine.add_verifier(verifier);
    }

    context.flow.permission_time_source.emplace(runtime::utc_now());
    context.flow.permission_resolution = context.dependencies.permission_service.resolve_permissions(
        "user:cli",
        context.flow.task_artifact.stream_key,
        *context.flow.permission_time_source,
        context.dependencies.permission_source);

    arcs::verification::VerificationContext verification_context{};
    verification_context.permissions = context.flow.permission_resolution->permissions;
    verification_context.policy = &context.flow.policy_current;
    verification_context.schema_registry = &context.dependencies.schema_registry;
    verification_context.store = &context.dependencies.store;
    verification_context.time_source = &*context.flow.permission_time_source;

    context.flow.report = verification_engine.run_all(context.flow.option, verification_context);
    context.dependencies.verification_service.append_unsupported_checks(context.flow.report, verification_plan.unsupported_checks);
    context.flow.report = arcs::verification::make_verification_report(context.flow.option, std::move(context.flow.report.checks));
    context.flow.report_artifact = context.dependencies.verification_service.make_named_report_artifact(
        context.flow.option,
        context.flow.report,
        "a_option_verification_report",
        "v_option_verification_report");

    if (context.flow.report.status == arcs::verification::CheckStatus::Pass) {
        context.logger.ok("verification_report", "pass | checks=" + std::to_string(context.flow.report.checks.size()));
        return {.status = StageStatus::Continue};
    }

    context.logger.fail("verification_report", arcs::verification::to_string(context.flow.report.status));
    context.flow.reason = runtime::first_blocker_or(context.flow.report, "verification blocked");
    context.logger.fail("decision", context.flow.reason);
    return {.status = StageStatus::Blocked, .reason = context.flow.reason};
}

} // namespace arcs::core::stages

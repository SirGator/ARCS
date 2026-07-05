/**
 * @file interpretation_stage.cpp
 * @brief Implements the pipeline stage that turns raw input into a parsed
 *        decision context. For free-text input it delegates to an external
 *        interpretation service, builds and verifies an interpretation
 *        proposal artifact, and blocks the flow if interpretation or its
 *        verification fails. For structured (non free-text) input it derives
 *        the parsed decision flags directly from demo flow options.
 */

#include "core/stages/interpretation_stage.hpp"

#include <memory>
#include <sstream>

namespace arcs::core::stages {
namespace {

/**
 * @brief Builds a ParsedInput from the demo flow options, or a default
 *        (all-false) ParsedInput if demo controls are disabled.
 * @param options Flow options controlling whether demo controls are enabled
 *        and, if so, their approval/permission/policy-drift values.
 * @return A ParsedInput reflecting the demo control settings.
 */
arcs::core::services::ParsedInput parse_demo_options(const arcs::core::FlowOptions& options)
{
    if (!options.enable_demo_controls) {
        return {};
    }

    return arcs::core::services::ParsedInput{
        .approval_yes = options.demo_approval_granted,
        .permission_yes = options.demo_permission_granted,
        .policy_drift = options.demo_policy_drift,
    };
}

} // namespace

/**
 * @brief For free-text input, interprets the input via the external
 *        interpretation service, builds an interpretation proposal artifact,
 *        verifies it against the schema registry, and blocks on failure;
 *        for non free-text input, derives parsed input from demo options.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the interpretation artifact, interpretation verification
 *        report, and parsed input.
 * @return StageResult::Blocked if free-text interpretation is unavailable or
 *         fails verification, otherwise StageResult::Continue.
 */
StageResult InterpretationStage::run(runtime::RuntimeContext& context) const
{
    if (context.free_text) {
        context.logger.ok("parse input", "free text routed through ingress and external interpretation artifact");

        const auto outcome = context.dependencies.interpretation_service.interpret(
            context.input,
            context.interpretation_config,
            context.dependencies.schema_registry,
            context.logger);
        if (!outcome.log_output.empty()) {
            context.messages.push_back(outcome.log_output);
        }

        if (!outcome.ok || !outcome.payload.has_value()) {
            runtime::add_version(context.flow.persisted_bundle, context.flow.ingress_event);
            context.flow.reason = "free text interpretation unavailable";
            return {.status = StageStatus::Blocked, .reason = context.flow.reason};
        }

        context.flow.interpretation_artifact = context.dependencies.interpretation_service.make_proposal_artifact(
            context.flow.ingress_event,
            *outcome.payload);
        context.logger.ok(
            "interpretation_proposal",
            "artifact created | artifact_id=" + context.flow.interpretation_artifact->artifact_id +
            " version=" + context.flow.interpretation_artifact->version_id);

        arcs::verification::VerificationEngine interpretation_verification_engine;
        interpretation_verification_engine.add_verifier(std::make_shared<arcs::verification::SchemaVerifier>());

        arcs::verification::VerificationContext interpretation_verification_context{};
        interpretation_verification_context.schema_registry = &context.dependencies.schema_registry;

        const auto interpretation_report = interpretation_verification_engine.run_all(
            *context.flow.interpretation_artifact,
            interpretation_verification_context);
        context.flow.interpretation_report_artifact = context.dependencies.verification_service.make_named_report_artifact(
            *context.flow.interpretation_artifact,
            interpretation_report,
            "a_interpretation_verification_report",
            "v_interpretation_verification_report");

        if (interpretation_report.status != arcs::verification::CheckStatus::Pass) {
            context.logger.fail(
                "interpretation_verification_report",
                arcs::verification::to_string(interpretation_report.status));
            runtime::add_version(context.flow.persisted_bundle, context.flow.ingress_event);
            runtime::add_version(context.flow.persisted_bundle, *context.flow.interpretation_artifact);
            runtime::add_version(context.flow.persisted_bundle, *context.flow.interpretation_report_artifact);
            context.flow.reason = "interpretation verification blocked";
            return {.status = StageStatus::Blocked, .reason = context.flow.reason};
        }

        context.logger.ok(
            "interpretation_verification_report",
            "pass | checks=" + std::to_string(interpretation_report.checks.size()));
        context.messages.push_back("interpretation: external worker accepted\n");
        context.flow.parsed_input = {};
    } else {
        context.flow.parsed_input = parse_demo_options(context.options);
    }

    context.logger.ok(
        "parse input",
        std::string(context.options.enable_demo_controls ? "demo control parsed | approval=" :
                        "demo control disabled | approval=") +
        (context.flow.parsed_input.approval_yes ? "yes" : "no") +
        " permission=" + (context.flow.parsed_input.permission_yes ? "yes" : "no") +
        " policy_drift=" + (context.flow.parsed_input.policy_drift ? "yes" : "no") +
        (context.free_text ? " | raw input treated as free text" : " | raw key-value input has no authority"));
    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

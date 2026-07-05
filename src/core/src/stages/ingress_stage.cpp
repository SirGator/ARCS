/**
 * @file ingress_stage.cpp
 * @brief Implements the first pipeline stage, which runs raw input through
 *        the ingress service (including quarantine checks) and produces the
 *        ingress event artifact that downstream stages consume.
 */

#include "core/stages/ingress_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Runs the raw input through the ingress service, rejecting it into
 *        quarantine on failure or recording the resulting ingress event
 *        artifact on success.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the ingress event artifact on success.
 * @return StageResult::Blocked if ingress rejects the input, otherwise
 *         StageResult::Continue.
 */
StageResult IngressStage::run(runtime::RuntimeContext& context) const
{
    auto ingress_result = context.dependencies.ingress_service.run(context.input, context.flow.quarantine);

    if (!ingress_result.success) {
        context.logger.fail(
            "ingress",
            ingress_result.rejection_reason + " (stage: " + ingress_result.rejection_stage + ")");
        context.flow.reason = "ingress rejected: " + ingress_result.rejection_reason;
        return {.status = StageStatus::Blocked, .reason = context.flow.reason};
    }

    context.flow.ingress_event = ingress_result.ingress_artifact;
    context.logger.ok(
        "ingress_event",
        "artifact created | stream_key=" + context.flow.ingress_event.stream_key +
        " source=" + context.flow.ingress_event.source.kind + "/" + context.flow.ingress_event.source.ref);
    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

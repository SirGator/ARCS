/**
 * @file task_stage.cpp
 * @brief Implements the pipeline stage that creates the task artifact from
 *        the ingress event, raw input, and parsed input.
 */

#include "core/stages/task_stage.hpp"

namespace arcs::core::stages {

/**
 * @brief Creates a task artifact from the ingress event, raw input, and
 *        parsed input, and records it on the pipeline context.
 * @param context Mutable runtime context carrying the pipeline state; updated
 *        with the newly created task artifact.
 * @return StageResult::Continue; this stage does not block the flow.
 */
StageResult TaskStage::run(runtime::RuntimeContext& context) const
{
    context.flow.task_artifact = context.dependencies.task_service.create_task(
        context.flow.ingress_event,
        context.input,
        context.flow.parsed_input);

    context.logger.ok(
        "task",
        "artifact created | task_id=" + context.flow.task_artifact.artifact_id +
        " version=" + context.flow.task_artifact.version_id);
    return {.status = StageStatus::Continue};
}

} // namespace arcs::core::stages

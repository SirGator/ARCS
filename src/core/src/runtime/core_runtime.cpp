/**
 * @file core_runtime.cpp
 * @brief Implements CoreRuntime, the top-level orchestrator that drives a single
 *        governance request through the ARCS pipeline stages (ingress,
 *        interpretation, task, planning, verification, action, approval,
 *        execution) and persists the resulting artifacts.
 *
 * This file also contains the logic for resuming a flow from a previously
 * issued approval artifact (promoting the pending action candidate, verifying
 * it, and executing it), plus helper functions used to assemble and commit
 * the batch of artifacts produced by a run.
 */

#include "core/runtime/core_runtime.hpp"

#include "core/runtime/runtime_context.hpp"
#include "core/runtime/runtime_support.hpp"

namespace arcs::core::runtime {

/**
 * @brief Default constructor. Uses the runtime's own in-memory store and the
 *        lazily-built default payload schema registry.
 */
CoreRuntime::CoreRuntime()
    : store_ref_(&store_)
    , schema_registry_(&default_payload_schema_registry())
{
    policy_repository_ = std::make_unique<arcs::core::services::FallbackPolicyRepository>(
        std::make_unique<arcs::core::services::StorePolicyRepository>(*store_ref_),
        std::make_unique<arcs::core::services::BootstrapPolicyRepository>());
    policy_service_ = std::make_unique<arcs::core::services::PolicyService>(*policy_repository_);
}

/**
 * @brief Constructs a CoreRuntime backed by an externally supplied store and
 *        schema registry, allowing callers to inject their own storage
 *        backend and schema set instead of the defaults.
 * @param store The artifact/event store the runtime will read from and write to.
 * @param schema_registry The payload schema registry used for validation.
 */
CoreRuntime::CoreRuntime(arcs::store::IStore& store, const arcs::schema::SchemaRegistry& schema_registry)
    : store_ref_(&store)
    , schema_registry_(&schema_registry)
{
    policy_repository_ = std::make_unique<arcs::core::services::FallbackPolicyRepository>(
        std::make_unique<arcs::core::services::StorePolicyRepository>(*store_ref_),
        std::make_unique<arcs::core::services::BootstrapPolicyRepository>());
    policy_service_ = std::make_unique<arcs::core::services::PolicyService>(*policy_repository_);
}

/**
 * @brief Runs a fresh governance flow for a raw request: builds a
 *        RuntimeContext, short-circuits with an "empty input" failure if the
 *        request has no input text, and otherwise executes the pipeline
 *        stages in order (ingress, interpretation, task, planning,
 *        verification, action, approval, execution), stopping early and
 *        persisting/finalizing as soon as any stage signals it should not
 *        continue.
 * @param request The incoming request containing input text, interpretation
 *        config, and flow options.
 * @return The FlowResult describing the outcome of the run.
 */
FlowResult CoreRuntime::run(const CoreRequest& request)
{
    const arcs::core::services::StorePermissionSource store_permission_source(*store_ref_);
    const arcs::core::services::DemoPermissionSource demo_permission_source;
    const arcs::core::services::CompositePermissionSource demo_and_store_permission_source(
        store_permission_source,
        demo_permission_source);
    const auto& permission_source = request.options.demo_permission_granted
        ? static_cast<const arcs::core::services::IPermissionSource&>(demo_and_store_permission_source)
        : static_cast<const arcs::core::services::IPermissionSource&>(store_permission_source);

    RuntimeContext context(
        request.input,
        request.interpretation_config,
        request.options,
        *store_ref_,
        *schema_registry_,
        ingress_service_,
        verification_service_,
        approval_service_,
        action_service_,
        decision_service_,
        execution_service_,
        permission_service_,
        permission_source,
        interpretation_service_,
        task_service_,
        planning_service_,
        *policy_service_,
        commit_service_,
        resume_service_);
    context.logger.ok(
        "input received",
        context.input.empty() ? "empty" : "text present | bytes=" + std::to_string(context.input.size()));

    if (context.input.empty()) {
        context.logger.fail("parse input", "empty input");
        context.flow.reason = "empty input";
        return finalize_flow_result(context, std::string{});
    }

    return run_fresh_flow(context);
}

/**
 * @brief Runs a flow starting from an artifact rather than raw text. If the
 *        artifact is not an approval, its raw_text payload is forwarded to
 *        the CoreRequest overload of run(). If it is an approval, this
 *        resumes the previously paused flow: it looks up the pending action
 *        candidate and option via the resume service, re-resolves
 *        permissions, promotes the action candidate to an approved action,
 *        re-verifies the action, executes it, and records the final
 *        decision, persisting/finalizing at each failure point along the way.
 * @param input_artifact The artifact that triggered this run (e.g. an
 *        approval artifact submitted by a user, or another artifact type
 *        whose raw_text payload is treated as free-text input).
 * @param options Flow options to apply to this run.
 * @return The FlowResult describing the outcome of the run.
 */
FlowResult CoreRuntime::run(const arcs::artifact::ArtifactVersion& input_artifact, const FlowOptions& options)
{
    if (input_artifact.type != "approval") {
        return run(CoreRequest{
            .input = input_artifact.payload.value("raw_text", std::string{}),
            .interpretation_config = nullptr,
            .options = options,
        });
    }

    const arcs::core::services::StorePermissionSource store_permission_source(*store_ref_);

    RuntimeContext context(
        std::string{"approval artifact: "} + input_artifact.artifact_id,
        nullptr,
        options,
        *store_ref_,
        *schema_registry_,
        ingress_service_,
        verification_service_,
        approval_service_,
        action_service_,
        decision_service_,
        execution_service_,
        permission_service_,
        store_permission_source,
        interpretation_service_,
        task_service_,
        planning_service_,
        *policy_service_,
        commit_service_,
        resume_service_);

    return resume_approval_flow(context, input_artifact);
}

} // namespace arcs::core::runtime

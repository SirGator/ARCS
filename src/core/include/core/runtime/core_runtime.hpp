/**
 * @file core_runtime.hpp
 * @brief Top-level orchestrator that wires together all core services and
 *        runs the full ingress-to-decision flow.
 */

#pragma once

#include <memory>

#include "artifact/artifact.hpp"
#include "core/flow_result.hpp"
#include "core/flow.hpp"
#include "core/runtime/core_request.hpp"
#include "core/services/approval_service.hpp"
#include "core/services/action_service.hpp"
#include "core/services/decision_service.hpp"
#include "core/services/execution_service.hpp"
#include "core/services/ingress_service.hpp"
#include "core/services/interpretation_service.hpp"
#include "core/services/permission_service.hpp"
#include "core/services/planning_service.hpp"
#include "core/services/policy_service.hpp"
#include "core/services/task_service.hpp"
#include "core/services/verification_service.hpp"
#include "core/commit/commit_service.hpp"
#include "core/resume/resume_service.hpp"
#include "schema/schema_registry.hpp"
#include "store/store.hpp"
#include "store/store_memory.hpp"

namespace arcs::core::runtime {

/**
 * @brief Owns the full set of core services (ingress, verification,
 *        approval, action, decision, execution, permission, interpretation,
 *        task, planning, policy, commit, resume) and drives them through a
 *        single flow run, either against an internal in-memory store or an
 *        externally supplied store and schema registry.
 */
class CoreRuntime {
public:
    /**
     * @brief Constructs a runtime backed by its own in-memory store.
     */
    CoreRuntime();

    /**
     * @brief Constructs a runtime backed by an externally owned store and
     *        schema registry.
     * @param store Store used to persist and resolve artifacts.
     * @param schema_registry Schema registry used to validate artifacts.
     */
    CoreRuntime(arcs::store::IStore& store, const arcs::schema::SchemaRegistry& schema_registry);

    /**
     * @brief Runs the full flow for a raw-input core request.
     * @param request Input text, interpretation config, and flow options.
     * @return The resulting flow outcome.
     */
    FlowResult run(const CoreRequest& request);

    /**
     * @brief Runs the full flow starting from an already-ingested artifact.
     * @param input_artifact Artifact version to use as the flow's input.
     * @param options Flow options.
     * @return The resulting flow outcome.
     */
    FlowResult run(
        const arcs::artifact::ArtifactVersion& input_artifact,
        const FlowOptions& options);

private:
    arcs::store::StoreMemory store_;
    arcs::store::IStore* store_ref_;
    const arcs::schema::SchemaRegistry* schema_registry_;
    arcs::core::services::IngressService ingress_service_;
    arcs::core::services::VerificationService verification_service_;
    arcs::core::services::ApprovalService approval_service_;
    arcs::core::services::ActionService action_service_;
    arcs::core::services::DecisionService decision_service_;
    arcs::core::services::ExecutionService execution_service_;
    arcs::core::services::PermissionService permission_service_;
    arcs::core::services::InterpretationService interpretation_service_;
    arcs::core::services::TaskService task_service_;
    arcs::core::services::PlanningService planning_service_;
    std::unique_ptr<arcs::core::services::IPolicyRepository> policy_repository_;
    std::unique_ptr<arcs::core::services::PolicyService> policy_service_;
    arcs::core::commit::CommitService commit_service_;
    arcs::core::resume::ResumeService resume_service_;
};

} // namespace arcs::core::runtime

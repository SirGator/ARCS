/**
 * @file runtime_context.hpp
 * @brief Mutable state threaded through the flow stages during a single
 *        `CoreRuntime` run, plus helper functions used to build artifacts
 *        and events along the way.
 */

#pragma once

#include <chrono>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "approval.hpp"
#include "artifact/artifact.hpp"
#include "core/flow.hpp"
#include "core/flow_result.hpp"
#include "core/services/approval_service.hpp"
#include "core/services/action_service.hpp"
#include "core/services/decision_service.hpp"
#include "core/services/common.hpp"
#include "core/services/execution_service.hpp"
#include "core/services/ingress_service.hpp"
#include "core/services/interpretation_service.hpp"
#include "core/services/permission_service.hpp"
#include "core/services/planning_service.hpp"
#include "core/services/policy_service.hpp"
#include "core/services/task_service.hpp"
#include "core/services/verification_service.hpp"
#include "core/system_logger.hpp"
#include "event/event.hpp"
#include "ingress/quarantine.hpp"
#include "interpretation/config.hpp"
#include "reducer/mock_time_source.hpp"
#include "schema/schema_registry.hpp"
#include "store/commit.hpp"
#include "store/store.hpp"
#include "verification/verifier.hpp"

#include "core/commit/commit_service.hpp"
#include "core/resume/resume_service.hpp"

namespace arcs::core::runtime {

struct RuntimeDependencies {
    arcs::store::IStore& store;
    const arcs::schema::SchemaRegistry& schema_registry;
    arcs::core::services::IngressService& ingress_service;
    arcs::core::services::VerificationService& verification_service;
    arcs::core::services::ApprovalService& approval_service;
    arcs::core::services::ActionService& action_service;
    arcs::core::services::DecisionService& decision_service;
    arcs::core::services::ExecutionService& execution_service;
    arcs::core::services::PermissionService& permission_service;
    const arcs::core::services::IPermissionSource& permission_source;
    arcs::core::services::InterpretationService& interpretation_service;
    arcs::core::services::TaskService& task_service;
    arcs::core::services::PlanningService& planning_service;
    arcs::core::services::PolicyService& policy_service;
    arcs::core::commit::CommitService& commit_service;
    arcs::core::resume::ResumeService& resume_service;
};

struct FlowState {
    PendingState pending_state{};
    FlowStatus status{FlowStatus::Blocked};
    std::string reason;

    arcs::ingress::QuarantineStore quarantine;
    arcs::store::commit::CommitBundle persisted_bundle{};

    arcs::artifact::ArtifactVersion ingress_event;
    std::optional<arcs::artifact::ArtifactVersion> interpretation_artifact;
    std::optional<arcs::artifact::ArtifactVersion> interpretation_report_artifact;
    arcs::core::services::ParsedInput parsed_input{};
    arcs::artifact::ArtifactVersion task_artifact;
    arcs::artifact::ArtifactVersion policy_current;
    arcs::artifact::ArtifactVersion policy_previous;
    arcs::artifact::ArtifactVersion option;
    std::optional<arcs::core::services::PermissionResolution> permission_resolution;
    std::optional<arcs::reducer::MockTimeSource> permission_time_source;
    arcs::verification::VerificationReportData report{};
    arcs::artifact::ArtifactVersion report_artifact;
    arcs::artifact::ArtifactVersion approval_request_artifact;
    arcs::artifact::ArtifactVersion approval_artifact;
    arcs::artifact::ArtifactVersion action_candidate_artifact;
    arcs::artifact::ArtifactVersion action_artifact;
    arcs::artifact::ArtifactVersion action_report_artifact;
    arcs::artifact::ArtifactVersion execution_result_artifact;
};

/**
 * @brief Holds all state a single flow run accumulates as it passes through
 *        the ingress, interpretation, task, policy, verification, approval,
 *        action, permission, and execution stages: references to the
 *        services and stores in use, the artifacts produced at each step,
 *        and the running log/decision/pending state.
 */
struct RuntimeContext {
    /**
     * @brief Constructs the context, binding it to the input, options, and
     *        the service/store instances owned by the runtime for the
     *        duration of one flow run.
     */
    RuntimeContext(
        std::string input_value,
        const arcs::interpretation::InterpretationApiConfig* interpretation_config_value,
        const FlowOptions& options_value,
        arcs::store::IStore& store_value,
        const arcs::schema::SchemaRegistry& schema_registry_value,
        arcs::core::services::IngressService& ingress_service_value,
        arcs::core::services::VerificationService& verification_service_value,
        arcs::core::services::ApprovalService& approval_service_value,
        arcs::core::services::ActionService& action_service_value,
        arcs::core::services::DecisionService& decision_service_value,
        arcs::core::services::ExecutionService& execution_service_value,
        arcs::core::services::PermissionService& permission_service_value,
        const arcs::core::services::IPermissionSource& permission_source_value,
        arcs::core::services::InterpretationService& interpretation_service_value,
        arcs::core::services::TaskService& task_service_value,
        arcs::core::services::PlanningService& planning_service_value,
        arcs::core::services::PolicyService& policy_service_value,
        arcs::core::commit::CommitService& commit_service_value,
        arcs::core::resume::ResumeService& resume_service_value);

    std::string input;
    const arcs::interpretation::InterpretationApiConfig* interpretation_config{nullptr};
    FlowOptions options{};
    bool free_text{false};

    SystemLogger logger;
    std::vector<std::string> messages;
    RuntimeDependencies dependencies;
    FlowState flow;
};

/**
 * @brief Converts the accumulated runtime context into the caller-facing
 *        `FlowResult`, rendering logger entries as diagnostics.
 * @param context Context accumulated over the flow run.
 * @return The finalized flow result.
 */
FlowResult finalize_flow_result(const RuntimeContext& context, const std::string& artifacts_dir);

/**
 * @brief Returns the current UTC time formatted as used throughout the flow.
 * @return Current UTC timestamp string.
 */
std::string utc_now();

/**
 * @brief Formats a given time point as a UTC timestamp string.
 * @param time_point Time point to format.
 * @return Formatted UTC timestamp string.
 */
std::string utc_at(std::chrono::system_clock::time_point time_point);

/**
 * @brief Returns a UTC timestamp string offset a number of hours from now.
 * @param hours Number of hours to add to the current time.
 * @return Formatted UTC timestamp string.
 */
std::string utc_after_hours(int hours);

/**
 * @brief Constructs a new artifact version populated with the given
 *        type/schema/provenance fields.
 * @param type Artifact type.
 * @param schema_id Schema identifier the artifact conforms to.
 * @param stream_key Store stream key the artifact belongs to.
 * @param actor_type Type of actor that produced the artifact.
 * @param actor_id Identifier of the actor that produced the artifact.
 * @param source_kind Kind of source the artifact originated from.
 * @param source_ref Reference to the originating source.
 * @param trust_level Trust level assigned to the artifact.
 * @param trust_source_class Trust source classification for the artifact.
 * @return The newly constructed artifact version.
 */
arcs::artifact::ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class);

/**
 * @brief Builds the semantic event that records a stream head advancing to
 *        the given artifact.
 * @param artifact Artifact the stream head advanced to.
 * @return The constructed event.
 */
arcs::event::Event make_head_advanced_event(const arcs::artifact::ArtifactVersion& artifact);

/**
 * @brief Adds an artifact version to a commit bundle's version list.
 * @param bundle Bundle to add the artifact to.
 * @param artifact Artifact version to add.
 */
void add_version(arcs::store::commit::CommitBundle& bundle, const arcs::artifact::ArtifactVersion& artifact);

/**
 * @brief Appends the contents of one commit bundle onto another.
 * @param destination Bundle that receives the appended contents.
 * @param source Bundle whose contents are appended.
 */
void append_bundle(
    arcs::store::commit::CommitBundle& destination,
    const arcs::store::commit::CommitBundle& source);

/**
 * @brief Returns the first blocking issue named in a verification report, or
 *        a fallback string if none is present.
 * @param report Verification report to inspect.
 * @param fallback Value to return if no blocker is found.
 * @return The first blocker's description, or @p fallback.
 */
std::string first_blocker_or(const arcs::verification::VerificationReportData& report, const std::string& fallback);

} // namespace arcs::core::runtime

/**
 * @file runtime_context.cpp
 * @brief Implements `RuntimeContext` construction and the free helper
 *        functions used across the flow stages to build artifacts and
 *        events (timestamps, artifact factories, decision/interpretation
 *        artifacts, commit bundle manipulation, and error-message lookup).
 */

#include "core/runtime/runtime_context.hpp"

#include <chrono>
#include <ctime>
#include <iomanip>
#include <memory>
#include <optional>
#include <sstream>

#include "artifact/factory.hpp"
#include "artifact/ids.hpp"
#include "core/services/common.hpp"
#include "event/event.hpp"

namespace arcs::core::runtime {
namespace {

using ArtifactVersion = arcs::artifact::ArtifactVersion;
using Event = arcs::event::Event;
using EventRef = arcs::event::EventRef;

DiagnosticSeverity to_diagnostic_severity(const StepStatus status)
{
    return status == StepStatus::Ok ? DiagnosticSeverity::Info : DiagnosticSeverity::Error;
}

} // namespace

/**
 * @brief Constructs the per-run context, binding it to the raw input string,
 *        flow options, and the shared store/schema registry/service
 *        instances that the flow stages will use for the duration of one
 *        run. Also determines whether the input looks like free text
 *        (no `=` present) and reserves capacity in the persisted bundle.
 * @param input_value Raw input string for this run (either free text or a
 *        structured `key=value` style command).
 * @param interpretation_config_value Optional configuration for the
 *        interpretation API; may be null if interpretation is not used.
 * @param options_value Flow-wide options controlling how the run behaves.
 * @param store_value Backing store used to read/write artifacts and events.
 * @param schema_registry_value Registry of known artifact schemas.
 * @param ingress_service_value Service handling the ingress stage.
 * @param verification_service_value Service handling the verification stage.
 * @param approval_service_value Service handling the approval stage.
 * @param action_service_value Service handling the action stage.
 * @param decision_service_value Service handling the decision stage.
 * @param execution_service_value Service handling the execution stage.
 * @param permission_service_value Service handling permission resolution.
 * @param interpretation_service_value Service handling the interpretation stage.
 * @param task_service_value Service handling the task stage.
 * @param planning_service_value Service handling the planning stage.
 * @param policy_service_value Service handling the policy stage.
 * @param commit_service_value Service used to commit persisted bundles.
 * @param resume_service_value Service used to resume a previously paused run.
 */
RuntimeContext::RuntimeContext(
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
    arcs::core::resume::ResumeService& resume_service_value)
    : input(std::move(input_value))
    , interpretation_config(interpretation_config_value)
    , options(options_value)
    , free_text(input.find('=') == std::string::npos)
    , dependencies{
        .store = store_value,
        .schema_registry = schema_registry_value,
        .ingress_service = ingress_service_value,
        .verification_service = verification_service_value,
        .approval_service = approval_service_value,
        .action_service = action_service_value,
        .decision_service = decision_service_value,
        .execution_service = execution_service_value,
        .permission_service = permission_service_value,
        .permission_source = permission_source_value,
        .interpretation_service = interpretation_service_value,
        .task_service = task_service_value,
        .planning_service = planning_service_value,
        .policy_service = policy_service_value,
        .commit_service = commit_service_value,
        .resume_service = resume_service_value,
    }
{
    flow.persisted_bundle.versions.reserve(12);
}

/**
 * @brief Builds the public FlowResult summary returned to callers of the
 *        runtime, copying the relevant fields out of the internal
 *        RuntimeContext into a typed FlowResult with diagnostics.
 * @param context The runtime context holding the accumulated run state.
 * @param artifacts_dir Filesystem path where this run's artifacts were written.
 * @return A FlowResult describing the outcome of the run.
 */
FlowResult finalize_flow_result(const RuntimeContext& context, const std::string& artifacts_dir)
{
    std::vector<Diagnostic> diagnostics;
    diagnostics.reserve(context.logger.entries().size() + context.messages.size());

    for (const auto& entry : context.logger.entries()) {
        diagnostics.push_back(Diagnostic{
            .code = entry.status == StepStatus::Ok ? "step.ok" : "step.fail",
            .severity = to_diagnostic_severity(entry.status),
            .message = entry.detail,
            .stage = entry.name,
        });
    }

    for (const auto& message : context.messages) {
        diagnostics.push_back(Diagnostic{
            .code = "note",
            .severity = DiagnosticSeverity::Info,
            .message = message,
        });
    }

    if (!artifacts_dir.empty()) {
        diagnostics.push_back(Diagnostic{
            .code = "artifacts.dir",
            .severity = DiagnosticSeverity::Info,
            .message = artifacts_dir,
            .stage = "artifacts",
        });
    }

    return FlowResult{
        .input = context.input,
        .status = context.flow.status,
        .reason = context.flow.reason,
        .pending = context.flow.pending_state.kind.empty()
            ? std::nullopt
            : std::optional<PendingState>{context.flow.pending_state},
        .diagnostics = std::move(diagnostics),
    };
}

/**
 * @brief Returns the current UTC time formatted as an ISO-8601 string.
 * @return The current time in `%Y-%m-%dT%H:%M:%SZ` format.
 */
std::string utc_now()
{
    return utc_at(std::chrono::system_clock::now());
}

/**
 * @brief Formats a given time point as UTC in ISO-8601 style.
 * @param time_point The time point to format.
 * @return The formatted time string in `%Y-%m-%dT%H:%M:%SZ` format.
 */
std::string utc_at(const std::chrono::system_clock::time_point time_point)
{
    const auto now_time_t = std::chrono::system_clock::to_time_t(time_point);
    std::tm tm{};

#if defined(_WIN32)
    gmtime_s(&tm, &now_time_t);
#else
    gmtime_r(&now_time_t, &tm);
#endif

    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%SZ");
    return out.str();
}

/**
 * @brief Returns a UTC timestamp offset a given number of hours into the
 *        future from now (used e.g. for approval expiry timestamps).
 * @param hours The number of hours to add to the current time.
 * @return The formatted future time string in `%Y-%m-%dT%H:%M:%SZ` format.
 */
std::string utc_after_hours(const int hours)
{
    return utc_at(std::chrono::system_clock::now() + std::chrono::hours(hours));
}

/**
 * @brief Constructs a base artifact stamped with the current UTC time,
 *        delegating the actual field population to the artifact factory.
 * @param type The artifact's type name (e.g. "decision").
 * @param schema_id The identifier of the payload schema this artifact conforms to.
 * @param stream_key The stream the artifact belongs to.
 * @param actor_type The type of actor that created the artifact (e.g. "system").
 * @param actor_id The identifier of the actor that created the artifact.
 * @param source_kind The kind of source that produced the artifact (e.g. "internal").
 * @param source_ref A reference describing the specific source of the artifact.
 * @param trust_level The trust level assigned to the artifact.
 * @param trust_source_class The trust source classification for the artifact.
 * @return The newly constructed artifact version.
 */
ArtifactVersion make_artifact(
    const std::string& type,
    const std::string& schema_id,
    const std::string& stream_key,
    const std::string& actor_type,
    const std::string& actor_id,
    const std::string& source_kind,
    const std::string& source_ref,
    const std::string& trust_level,
    const std::string& trust_source_class)
{
    return arcs::artifact::factory::make_base_artifact(
        type,
        schema_id,
        stream_key,
        actor_type,
        actor_id,
        source_kind,
        source_ref,
        trust_level,
        trust_source_class,
        utc_now());
}

/**
 * @brief Builds an "interpretation_proposal" artifact wrapping the payload
 *        produced by the external interpretation worker, linking it back to
 *        the originating ingress event as its provenance parent.
 * @param ingress_event The ingress event artifact this interpretation is derived from.
 * @param interpretation_payload The interpreted payload to store on the artifact.
 * @return The newly constructed interpretation proposal artifact.
 */
ArtifactVersion make_interpretation_proposal_artifact(
    const ArtifactVersion& ingress_event,
    const nlohmann::json& interpretation_payload)
{
    ArtifactVersion artifact = arcs::artifact::factory::make_base_artifact(
        "interpretation_proposal",
        "arcs.interpretation_proposal.v1",
        ingress_event.stream_key,
        "system",
        "interpretation_worker",
        "api",
        "interpret",
        "low",
        "external",
        utc_now());

    artifact.payload = interpretation_payload;
    artifact.provenance.parents = {ingress_event.artifact_id};
    artifact.provenance.rules_applied = {"external_interpretation"};
    artifact.provenance.transform = "interpret_free_text";
    return artifact;
}

/**
 * @brief Creates a "head_advanced" event referencing the given artifact as
 *        its target, used to record in the event log that an artifact's
 *        stream head has moved to a new version.
 * @param artifact The artifact version the new head points to.
 * @return The constructed head-advanced event.
 */
Event make_head_advanced_event(const ArtifactVersion& artifact)
{
    Event event{};
    event.event_id = arcs::artifact::ids::new_event_id();
    event.event_type = "head_advanced";
    event.ts = utc_now();
    event.actor = artifact.created_by;
    event.refs.push_back(EventRef{
        .artifact_id = artifact.artifact_id,
        .version_id = artifact.version_id,
        .role = "target",
    });
    event.stream_key = artifact.stream_key;
    event.payload = {
        {"artifact_type", artifact.type},
        {"schema_id", artifact.schema_id},
    };
    return event;
}

/**
 * @brief Adds an artifact to a commit bundle as a new pending version and
 *        appends a corresponding head-advanced event for it.
 * @param bundle The commit bundle to append the version and event to.
 * @param artifact The artifact version being added.
 */
void add_version(arcs::store::commit::CommitBundle& bundle, const ArtifactVersion& artifact)
{
    bundle.versions.push_back(arcs::store::commit::PendingVersion{artifact, std::nullopt});
    bundle.events.push_back(make_head_advanced_event(artifact));
}

/**
 * @brief Appends all versions and events from a source commit bundle onto a
 *        destination bundle, concatenating both in place.
 * @param destination The bundle that receives the appended versions and events.
 * @param source The bundle whose versions and events are copied.
 */
void append_bundle(
    arcs::store::commit::CommitBundle& destination,
    const arcs::store::commit::CommitBundle& source)
{
    destination.versions.insert(
        destination.versions.end(),
        source.versions.begin(),
        source.versions.end());
    destination.events.insert(
        destination.events.end(),
        source.events.begin(),
        source.events.end());
}

/**
 * @brief Returns the first blocking reason recorded in a verification
 *        report, or a fallback message if the report has no blockers.
 * @param report The verification report to inspect.
 * @param fallback The message to return when there are no blockers.
 * @return The first blocker string, or `fallback` if none exist.
 */
std::string first_blocker_or(const arcs::verification::VerificationReportData& report, const std::string& fallback)
{
    if (!report.blockers.empty()) {
        return report.blockers.front();
    }

    return fallback;
}

/**
 * @brief Builds the final "decision" artifact for a flow run, embedding the
 *        decision status, reason, a summary of the triggering verification
 *        report, and references to the related approval/action/execution
 *        artifacts (any of which may be empty if not applicable).
 * @param option The option artifact this decision is made about (used as provenance parent).
 * @param report The verification report data that led to this decision.
 * @param status The decision status (e.g. "blocked", "not_blocked").
 * @param reason A human-readable explanation for the decision.
 * @param approval_artifact_id Id of the related approval artifact, if any.
 * @param action_artifact_id Id of the related action artifact, if any.
 * @param execution_result_artifact_id Id of the related execution result artifact, if any.
 * @return The newly constructed decision artifact.
 */
ArtifactVersion make_decision_artifact(
    const ArtifactVersion& option,
    const arcs::verification::VerificationReportData& report,
    const std::string& status,
    const std::string& reason,
    const std::string& approval_artifact_id,
    const std::string& action_artifact_id,
    const std::string& execution_result_artifact_id)
{
    ArtifactVersion artifact = make_artifact(
        "decision",
        "arcs.decision.v1",
        option.stream_key,
        "system",
        "kernel",
        "internal",
        "decision",
        "high",
        "system");

    artifact.payload = nlohmann::json{
        {"status", status},
        {"reason", reason},
        {"verification_report", {
            {"artifact_id", report.target.artifact_id},
            {"version_id", report.target.version_id},
            {"status", arcs::verification::to_string(report.status)},
        }},
        {"approval_artifact_id", approval_artifact_id},
        {"action_artifact_id", action_artifact_id},
        {"execution_result_artifact_id", execution_result_artifact_id},
    };

    artifact.provenance.parents = {option.artifact_id};
    artifact.provenance.rules_applied = {"kernel_decision"};
    artifact.provenance.transform = "decide";
    return artifact;
}

/**
 * @brief Builds a short human-readable risk summary string combining the
 *        option's safety level and the action candidate's action type.
 * @param option The option artifact to read the safety level from.
 * @param action_candidate The action candidate artifact to read the action type from.
 * @return A summary string of the form "safety_level=...; action_type=...".
 */
std::string make_risk_summary(const ArtifactVersion& option, const ArtifactVersion& action_candidate)
{
    const auto safety_level = option.payload.value("safety_level", std::string{"unknown"});
    const auto action_type = action_candidate.payload.value("type", std::string{"unknown"});
    return "safety_level=" + safety_level + "; action_type=" + action_type;
}

} // namespace arcs::core::runtime

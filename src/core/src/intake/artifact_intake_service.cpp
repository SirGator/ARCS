#include "core/intake/artifact_intake_service.hpp"

#include <chrono>

#include "artifact/factory.hpp"
#include "core/runtime/runtime_context.hpp"

namespace arcs::core::intake {
namespace {

std::string trust_level_for(arcs::adapters::AdapterKind kind)
{
    switch (kind) {
    case arcs::adapters::AdapterKind::Input:
    case arcs::adapters::AdapterKind::ExternalState:
    case arcs::adapters::AdapterKind::Interpretation:
    case arcs::adapters::AdapterKind::Llm:
        return "low";
    case arcs::adapters::AdapterKind::Reasoning:
    case arcs::adapters::AdapterKind::Database:
        return "medium";
    case arcs::adapters::AdapterKind::Output:
        return "high";
    }

    return "low";
}

std::string trust_source_class_for(const AdapterSubmission& submission)
{
    if (!submission.actor_type.empty()) {
        return submission.actor_type;
    }
    if (!submission.source_kind.empty() && submission.source_kind == "external") {
        return "external";
    }
    return "system";
}

} // namespace

ArtifactIntakeService::ArtifactIntakeService(
    arcs::store::IStore& store,
    const arcs::schema::SchemaRegistry& schema_registry,
    const arcs::core::commit::CommitService& commit_service)
    : store_(store)
    , schema_gate_(schema_registry)
    , commit_service_(commit_service)
{
}

IntakeResult ArtifactIntakeService::accept(
    const AdapterSubmission& submission,
    arcs::ingress::QuarantineStore* quarantine) const
{
    auto artifact = arcs::artifact::factory::make_base_artifact(
        submission.artifact_type,
        submission.schema_id,
        submission.stream_key,
        submission.actor_type.empty() ? "system" : submission.actor_type,
        submission.actor_id.empty() ? submission.adapter_id : submission.actor_id,
        submission.source_kind.empty() ? "internal" : submission.source_kind,
        submission.source_ref.empty() ? submission.adapter_id : submission.source_ref,
        trust_level_for(submission.adapter_kind),
        trust_source_class_for(submission),
        arcs::core::runtime::utc_now());

    artifact.payload = submission.payload;
    if (submission.metadata.is_object() && !submission.metadata.empty()) {
        artifact.tags.push_back("adapter:" + submission.adapter_id);
    }
    provenance_builder_.apply(artifact, submission);

    const auto schema_result = schema_gate_.validate(submission.schema_id, submission.payload);
    if (!schema_result.valid) {
        std::string rejection_reason = "schema validation failed";
        if (!schema_result.errors.empty()) {
            rejection_reason += ": " + schema_result.errors.front();
        }
        return quarantine_policy_.reject(artifact, rejection_reason, "schema_gate", quarantine);
    }

    arcs::store::commit::CommitBundle persisted_bundle{};
    const auto commit_context = arcs::core::commit::CommitContext{
        .commit_id = "c_intake_" + artifact.version_id,
        .stage = "artifact_intake",
        .actor = submission.adapter_id,
        .reason = "adapter submission accepted",
    };
    commit_service_.commit_and_collect(
        store_,
        persisted_bundle,
        commit_context,
        "artifact_committed",
        {artifact},
        artifact.created_at);

    return IntakeResult{
        .accepted = true,
        .artifact_ref = ArtifactRef{.artifact_id = artifact.artifact_id, .version_id = artifact.version_id},
        .rejection_reason = {},
        .diagnostics = {arcs::core::Diagnostic{
            .code = "intake.accept",
            .severity = arcs::core::DiagnosticSeverity::Info,
            .message = "artifact accepted",
            .stage = "artifact_intake",
            .artifact_id = artifact.artifact_id,
        }},
    };
}

} // namespace arcs::core::intake

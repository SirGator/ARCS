//! Verify immutable parent versions and the atomic final-state link.
use super::*;
use crate::store::database::{ArtifactMetadata, MetadataField};
use crate::store::{ArtifactRelations, SqliteArtifactStore};

/// Checks original request, dispatch, final state and (for a response) result.
/// Untrusted artifacts with a similar payload are not execution evidence.
pub fn read(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    artifact: &Artifact,
) -> Result<ReasoningAttempt, ReasoningError> {
    read_with_target(store, schemas, artifact).map(|(record, _)| record)
}

pub(in crate::reasoning) fn read_with_target(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    artifact: &Artifact,
) -> Result<(ReasoningAttempt, Option<SchemaId>), ReasoningError> {
    validate_artifact(artifact, schemas)
        .map_err(|_| invalid("invalid reasoning attempt envelope"))?;
    if !trusted(artifact, PUBLISHER) || artifact.schema_id.0 != SCHEMA || artifact.version != 1 {
        return Err(invalid("untrusted reasoning attempt evidence"));
    }
    if store.get(&artifact.version_id)?.as_ref() != Some(artifact) {
        return Err(invalid(
            "attempt evidence differs from its stored immutable version",
        ));
    }
    let record: ReasoningAttempt = serde_json::from_value(artifact.payload.clone())?;
    if artifact.subject.as_ref() != Some(&capability_subject(&record.capability))
        || artifact.stream_key != stream(&record.invocation_id, &record.dispatch_version_id)
        || artifact.source.reference != artifact.stream_key
    {
        return Err(invalid("reasoning attempt identity changed"));
    }
    let dispatch = load(
        store,
        &record.dispatch_version_id,
        &[
            MetadataField::value("status", "$.payload.status"),
            MetadataField::value("kind", "$.payload.kind"),
            MetadataField::value("input_version", "$.payload.input_version"),
            MetadataField::value("invocation_id", "$.payload.invocation_id"),
            MetadataField::value("capability", "$.payload.capability"),
        ],
    )?;
    let request = load(
        store,
        &record.request_version_id,
        &[
            MetadataField::value("invocation_id", "$.payload.invocation.invocation_id"),
            MetadataField::value("request_id", "$.payload.invocation.request_id"),
            MetadataField::value("capability", "$.payload.invocation.capability"),
            MetadataField::value("method", "$.payload.invocation.method"),
            MetadataField::value("budget", "$.payload.invocation.budget"),
            MetadataField::value("target_schema_id", "$.payload.invocation.target_schema_id"),
        ],
    )?;
    if !trusted_metadata(&dispatch, "arcs.runtime.invocation")
        || dispatch.schema_id.0 != "arcs.invocation.v1"
        || dispatch.fields["status"] != "dispatched"
        || dispatch.fields["kind"] != "reasoning"
        || dispatch.fields["input_version"] != record.request_version_id.0
        || dispatch.fields["invocation_id"] != record.invocation_id
        || dispatch.fields["capability"]
            != format!(
                "{}/{}",
                record.capability.adapter_id.0, record.capability.capability_id.0
            )
        || !trusted_metadata(&request, "arcs.reasoning.v2")
        || request.schema_id.0 != "arcs.reasoning_request.v2"
    {
        return Err(invalid(
            "reasoning attempt lacks a trusted request and dispatch",
        ));
    }
    let invocation = &request.fields;
    if invocation["invocation_id"] != record.invocation_id
        || invocation["request_id"] != record.request_id
        || invocation["capability"] != serde_json::to_value(&record.capability)?
        || invocation["method"] != serde_json::to_value(record.method)?
        || invocation["budget"] != serde_json::to_value(&record.budget)?
    {
        return Err(invalid(
            "reasoning attempt does not match its original request",
        ));
    }
    let final_version = VersionId(format!(
        "{}:v{}",
        dispatch.artifact_id.0,
        dispatch.version + 1
    ));
    let final_state = load(
        store,
        &final_version,
        &[
            MetadataField::value("status", "$.payload.status"),
            MetadataField::value("invocation_id", "$.payload.invocation_id"),
            MetadataField::value("result_version", "$.payload.result_version"),
            MetadataField::prefix("retry_prefix", "$.payload.error", 10),
        ],
    )?;
    if !trusted_metadata(&final_state, "arcs.runtime.invocation")
        || final_state.schema_id != dispatch.schema_id
        || final_state.artifact_id != dispatch.artifact_id
        || final_state.fields["invocation_id"] != record.invocation_id
        || !ArtifactRelations::new(store)
            .outgoing(&final_version)?
            .iter()
            .any(|edge| edge.to == artifact.version_id && edge.kind == relation_kinds::based_on())
    {
        return Err(invalid(
            "reasoning attempt was not committed with its final state",
        ));
    }
    match &record.outcome {
        AttemptOutcome::Failed { retryable, .. } => {
            if final_state.fields["status"] != "failed"
                || final_state.fields["retry_prefix"]
                    .as_str()
                    .is_none_or(|error| error.starts_with("retryable:") != *retryable)
            {
                return Err(invalid(
                    "reasoning attempt failure does not match its final state",
                ));
            }
        }
        AttemptOutcome::Response {
            status,
            result_version_id,
            candidate_count,
            adapter_reported_iterations,
            adapter_reported_output_tokens,
        } => {
            let result = load(
                store,
                result_version_id,
                &[
                    MetadataField::value("invocation_id", "$.payload.response.invocation_id"),
                    MetadataField::value("request_id", "$.payload.response.request_id"),
                    MetadataField::value("status", "$.payload.response.status"),
                    MetadataField::array_length("candidate_count", "$.payload.response.candidates"),
                    MetadataField::value("iterations", "$.payload.response.trace.iterations"),
                    MetadataField::value("output_tokens", "$.payload.response.trace.output_tokens"),
                    MetadataField::value("method", "$.payload.response.trace.method"),
                ],
            )?;
            let response = &result.fields;
            if final_state.fields["status"] != "succeeded"
                || final_state.fields["result_version"] != result_version_id.0
                || !trusted_metadata(&result, "arcs.reasoning.v2")
                || result.schema_id.0 != "arcs.reasoning_result.v2"
                || response["invocation_id"] != record.invocation_id
                || response["request_id"] != record.request_id
                || response["method"] != serde_json::to_value(record.method)?
                || response["status"] != serde_json::to_value(status)?
                || response["candidate_count"].as_u64() != Some(*candidate_count as u64)
                || response["iterations"] != serde_json::to_value(adapter_reported_iterations)?
                || response["output_tokens"]
                    != serde_json::to_value(adapter_reported_output_tokens)?
            {
                return Err(invalid(
                    "reasoning attempt response does not match its final result",
                ));
            }
        }
    }
    for parent in parents(&record) {
        if artifact
            .provenance
            .as_ref()
            .is_none_or(|p| !p.parents.contains(&parent.0))
        {
            return Err(invalid("reasoning attempt provenance is incomplete"));
        }
    }
    Ok((
        record,
        invocation["target_schema_id"]
            .as_str()
            .map(|id| SchemaId(id.into())),
    ))
}

pub(in crate::reasoning) fn trusted(artifact: &Artifact, publisher: &str) -> bool {
    artifact.created_by.id == publisher
        && artifact.created_by.actor_type == ActorType::System
        && artifact.source.kind == SourceKind::Internal
        && artifact.trust.level == TrustLevel::High
        && artifact.trust.source_class == SourceClass::System
}
fn trusted_metadata(artifact: &ArtifactMetadata, publisher: &str) -> bool {
    artifact.created_by.id == publisher
        && artifact.created_by.actor_type == ActorType::System
        && artifact.source.kind == SourceKind::Internal
        && artifact.trust.level == TrustLevel::High
        && artifact.trust.source_class == SourceClass::System
}
fn load(
    store: &SqliteArtifactStore,
    id: &VersionId,
    fields: &[MetadataField<'_>],
) -> Result<ArtifactMetadata, ReasoningError> {
    store
        .metadata_projection(id, fields, 32_768)?
        .ok_or_else(|| invalid("missing reasoning attempt evidence parent"))
}

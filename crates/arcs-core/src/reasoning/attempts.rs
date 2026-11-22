//! Core-observed dispatch outcomes. Valid candidates are not verified task success.
use super::{
    ReasoningError, ReasoningInvocationV2, ReasoningMethod, ReasoningResources,
    ReasoningResponseV2, ReasoningStatus,
};
use crate::adapters::{AdapterCallError, CapabilityRef};
use crate::core::*;
use crate::runtime::{InvocationKind, InvocationState, deterministic_invocation_id};
use crate::store::relation_kinds;
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "arcs.reasoning_attempt.v1";
pub const PUBLISHER: &str = "arcs.reasoning.attempt";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningFailureKind {
    AdapterTimeout,
    AdapterUnavailable,
    AdapterRejected,
    InputData,
    Authorization,
    InvalidResponse,
    Core,
    Contract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptOutcome {
    Response {
        status: ReasoningStatus,
        result_version_id: VersionId,
        candidate_count: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        adapter_reported_iterations: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        adapter_reported_output_tokens: Option<usize>,
    },
    Failed {
        failure: ReasoningFailureKind,
        retryable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningAttempt {
    pub candidate_quality_verified: bool,
    pub invocation_id: String,
    pub dispatch_version_id: VersionId,
    pub request_version_id: VersionId,
    pub request_id: String,
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub budget: ReasoningResources,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_completion_latency_ms: Option<u64>,
    pub outcome: AttemptOutcome,
}

pub fn failure_kind(error: &ReasoningError) -> ReasoningFailureKind {
    use ReasoningError::*;
    match error {
        AdapterCall(AdapterCallError::Timeout) => ReasoningFailureKind::AdapterTimeout,
        AdapterCall(AdapterCallError::Unavailable(_)) => ReasoningFailureKind::AdapterUnavailable,
        AdapterCall(AdapterCallError::Rejected(_)) => ReasoningFailureKind::AdapterRejected,
        InputData(_) => ReasoningFailureKind::InputData,
        Authorization(_) => ReasoningFailureKind::Authorization,
        InvalidResponse(_)
        | InvalidPayload(_)
        | InvalidReasoningTrace
        | InvocationResponseMismatch
        | ResponseRequestMismatch
        | ResponseTooLarge { .. }
        | TooManyCandidates { .. }
        | UnexpectedCandidateSchema(_)
        | ForbiddenCandidateCapability(_)
        | CandidateReferenceOutsideContext(_) => ReasoningFailureKind::InvalidResponse,
        Store(_) | Invocation(_) | Serialization(_) => ReasoningFailureKind::Core,
        _ => ReasoningFailureKind::Contract,
    }
}

pub fn capability_subject(capability: &CapabilityRef) -> SubjectId {
    SubjectId(format!(
        "reasoning-attempt:{}:{}:{}:{}",
        capability.adapter_id.0.len(),
        capability.adapter_id.0,
        capability.capability_id.0.len(),
        capability.capability_id.0
    ))
}

fn stream(invocation: &str, dispatch: &VersionId) -> String {
    format!(
        "reasoning-attempt:{}",
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &["dispatch", invocation, &dispatch.0]
        )
    )
}

pub(super) fn response_outcome(
    response: &ReasoningResponseV2,
    result: &VersionId,
) -> AttemptOutcome {
    AttemptOutcome::Response {
        status: response.status,
        result_version_id: result.clone(),
        candidate_count: response.candidates.len(),
        adapter_reported_iterations: response.trace.iterations,
        adapter_reported_output_tokens: response.trace.output_tokens,
    }
}

pub(super) fn create(
    invocation: &ReasoningInvocationV2,
    state: &InvocationState,
    outcome: AttemptOutcome,
    latency: Option<u64>,
    schemas: &SchemaRegistry,
    ids: &mut dyn ArtifactIdGenerator,
    clock: &dyn Clock,
) -> Result<Artifact, ReasoningError> {
    let record = ReasoningAttempt {
        candidate_quality_verified: false,
        invocation_id: invocation.invocation_id.clone(),
        dispatch_version_id: state.version_id(),
        request_version_id: state.input_version.clone(),
        request_id: invocation.request_id.clone(),
        capability: invocation.capability.clone(),
        method: invocation.method,
        budget: invocation.budget.clone(),
        host_completion_latency_ms: latency,
        outcome,
    };
    let parents = parents(&record);
    let key = stream(&record.invocation_id, &record.dispatch_version_id);
    ArtifactFactory::new(clock, ids)
        .create(ArtifactFactoryInput {
            schema: schemas
                .get(&SchemaId(SCHEMA.into()))
                .ok_or_else(|| ReasoningError::MissingRegisteredSchema(SchemaId(SCHEMA.into())))?
                .clone(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: key.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: key,
            subject: capability_subject(&record.capability),
            tags: vec![],
            payload: serde_json::to_value(&record)?,
            provenance: Some(Provenance {
                parents: parents.into_iter().map(|id| id.0).collect(),
                rules_applied: vec!["reasoning.attempt.observed.v1".into()],
                ..Default::default()
            }),
        })
        .map_err(|_| invalid("cannot construct reasoning attempt evidence"))
}

fn parents(record: &ReasoningAttempt) -> Vec<VersionId> {
    let mut result = vec![
        record.request_version_id.clone(),
        record.dispatch_version_id.clone(),
    ];
    if let AttemptOutcome::Response {
        result_version_id, ..
    } = &record.outcome
    {
        result.push(result_version_id.clone());
    }
    result
}

pub(super) fn relations(
    artifact: &Artifact,
    invocation: &ReasoningInvocationV2,
) -> Result<Vec<(VersionId, crate::store::RelationKind)>, ReasoningError> {
    let record: ReasoningAttempt = serde_json::from_value(artifact.payload.clone())?;
    Ok(parents(&record)
        .into_iter()
        .map(|id| (id, relation_kinds::based_on()))
        .chain(
            invocation
                .context
                .iter()
                .map(|item| (item.version_id.clone(), relation_kinds::based_on())),
        )
        .collect())
}

fn invalid(message: &str) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}

mod history;
mod validation;
pub use history::*;
pub use validation::read;
pub(super) use validation::read_with_target;
pub(super) use validation::trusted;
#[cfg(test)]
mod tests;

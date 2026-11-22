//! Bounded, operator-authorized alternatives for one immutable Reason request.
//! A completed failure is evidence for a new request, never a new action grant.
mod records;
#[cfg(test)]
mod tests;
mod validation;
mod views;
pub use views::{RecoveryStatus, completed_choice, status};

use super::general::reasoning_invocation_id as invocation_id;
use super::selection::{ReasonerChoice, SelectionOwner, SelectionPolicy};
use super::{ReasoningError, ReasoningRequestV2};
use crate::adapters::AdapterRegistry;
use crate::core::*;
use crate::runtime::{
    InvocationKind, InvocationService, InvocationStatus, deterministic_invocation_id,
};
use crate::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use arcs_adapter_protocol::InputDataSelection;
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "arcs.reasoning_recovery.v1";
pub const STEP_SCHEMA: &str = "arcs.reasoning_recovery_step.v1";
pub const PUBLISHER: &str = "arcs.reasoning.recovery";
pub const CONSTRAINT: &str = "arcs_reasoning_recovery";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    pub alternatives: Vec<ReasonerChoice>,
    pub delay_ms: u64,
}
impl RecoveryPolicy {
    pub fn validate(
        &self,
        registry: &AdapterRegistry,
        schemas: &SchemaRegistry,
        target: &SchemaId,
    ) -> Result<(), ReasoningError> {
        if !(1..=7).contains(&self.alternatives.len()) || self.delay_ms > 60_000 {
            return Err(invalid(
                "recovery needs 1..7 alternatives and a delay of at most 60000 ms",
            ));
        }
        for choice in &self.alternatives {
            SelectionPolicy {
                target_schema_id: target.clone(),
                query_text: String::new(),
                choices: vec![choice.clone()],
                history: None,
            }
            .validate(registry, schemas)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryDefinition {
    pub owner: SelectionOwner,
    pub base_request: ReasoningRequestV2,
    pub inputs: Vec<InputDataSelection>,
    pub policy: RecoveryPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryStep {
    pub definition_version_id: VersionId,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_step: Option<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_version_id: Option<VersionId>,
    pub not_before_unix_ms: u64,
}
#[derive(Debug)]
pub struct RecoveryCall {
    pub request: ReasoningRequestV2,
    pub step_version_id: VersionId,
    pub definition_version_id: VersionId,
    pub index: usize,
}
#[derive(Debug)]
pub enum RecoveryPreparation {
    Ready(Box<RecoveryCall>),
    Waiting {
        not_before_unix_ms: u64,
    },
    Stopped {
        failure_version_id: VersionId,
        attempts: usize,
    },
}

pub struct RecoveryService<'a> {
    pub registry: &'a AdapterRegistry,
    pub schemas: &'a SchemaRegistry,
    pub store: &'a SqliteArtifactStore,
    pub ids: &'a mut dyn ArtifactIdGenerator,
    pub clock: &'a dyn Clock,
}
impl RecoveryService<'_> {
    pub fn prepare(
        &mut self,
        definition: &RecoveryDefinition,
        now_unix_ms: u64,
    ) -> Result<RecoveryPreparation, ReasoningError> {
        validation::definition(self.registry, self.schemas, self.store, definition)?;
        if now_unix_ms > i64::MAX as u64 {
            return Err(invalid("recovery time exceeds SQLite range"));
        }
        let artifact = records::persist_definition(self, definition)?;
        if validation::load_definition(
            self.registry,
            self.schemas,
            self.store,
            &artifact.version_id,
        )? != *definition
        {
            return Err(invalid("recovery definition changed"));
        }
        let mut previous = None;
        let mut failure = None;
        for index in 0..=definition.policy.alternatives.len() {
            let key = records::step_key(&artifact.version_id, index);
            let step_artifact = if let Some(saved) = self.store.find_by_stream_key(&key)? {
                saved
            } else {
                let not_before_unix_ms = if index == 0 {
                    0
                } else {
                    now_unix_ms
                        .checked_add(definition.policy.delay_ms)
                        .filter(|n| *n <= i64::MAX as u64)
                        .ok_or_else(|| invalid("recovery delay overflow"))?
                };
                records::persist_step(
                    self,
                    &RecoveryStep {
                        definition_version_id: artifact.version_id.clone(),
                        index,
                        previous_step: previous.clone(),
                        failure_version_id: failure.clone(),
                        not_before_unix_ms,
                    },
                )?
            };
            let (saved_definition, step) = validation::load(
                self.registry,
                self.schemas,
                self.store,
                &step_artifact.version_id,
            )?;
            if saved_definition != *definition
                || step.previous_step != previous
                || step.failure_version_id != failure
            {
                return Err(invalid("recovery history identity changed"));
            }
            if now_unix_ms < step.not_before_unix_ms {
                return Ok(RecoveryPreparation::Waiting {
                    not_before_unix_ms: step.not_before_unix_ms,
                });
            }
            let request = request(definition, &step, &step_artifact.version_id);
            let state = InvocationService::new(self.store, self.schemas, self.clock)
                .lookup(&invocation_id(&request))?;
            let Some(state) = state.filter(|state| state.status == InvocationStatus::Failed) else {
                return Ok(RecoveryPreparation::Ready(Box::new(RecoveryCall {
                    request,
                    step_version_id: step_artifact.version_id,
                    definition_version_id: artifact.version_id.clone(),
                    index,
                })));
            };
            let mut evidence = None;
            for edge in ArtifactRelations::new(self.store).outgoing(&state.version_id())? {
                if edge.kind != relation_kinds::based_on() {
                    continue;
                }
                let candidate = self
                    .store
                    .get(&edge.to)?
                    .ok_or_else(|| invalid("missing recovery failure evidence"))?;
                if candidate.schema_id.0 != super::attempts::SCHEMA {
                    continue;
                }
                let record = super::attempts::read(self.store, self.schemas, &candidate)?;
                validation::failure_for_step(
                    self.store,
                    self.schemas,
                    definition,
                    &step,
                    &step_artifact.version_id,
                    &candidate,
                )?;
                if evidence.replace((candidate.version_id, record)).is_some() {
                    return Err(invalid("ambiguous recovery failure evidence"));
                }
            }
            let (version, record) = evidence.ok_or_else(|| {
                invalid("failed recovery request has no completed outcome evidence")
            })?;
            if index == definition.policy.alternatives.len() || !eligible(&record.outcome) {
                return Ok(RecoveryPreparation::Stopped {
                    failure_version_id: version,
                    attempts: index + 1,
                });
            }
            previous = Some(step_artifact.version_id);
            failure = Some(version);
        }
        unreachable!("bounded recovery always returns a call or terminal outcome")
    }
}

fn eligible(outcome: &super::attempts::AttemptOutcome) -> bool {
    use super::attempts::{AttemptOutcome, ReasoningFailureKind};
    matches!(
        outcome,
        AttemptOutcome::Failed {
            failure: ReasoningFailureKind::AdapterTimeout
                | ReasoningFailureKind::AdapterUnavailable,
            retryable: true
        }
    )
}

fn choice(definition: &RecoveryDefinition, index: usize) -> ReasonerChoice {
    if index == 0 {
        ReasonerChoice {
            capability: definition.base_request.capability.clone(),
            method: definition.base_request.method,
            budget: definition.base_request.budget.clone(),
        }
    } else {
        definition.policy.alternatives[index - 1].clone()
    }
}
fn request(
    definition: &RecoveryDefinition,
    step: &RecoveryStep,
    version: &VersionId,
) -> ReasoningRequestV2 {
    let mut result = definition.base_request.clone();
    let selected = choice(definition, step.index);
    result.capability = selected.capability;
    result.method = selected.method;
    result.budget = selected.budget;
    if step.index > 0 {
        result.request_id = deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                "recovery",
                &step.definition_version_id.0,
                &step.index.to_string(),
            ],
        );
    }
    if !result.constraints.is_object() {
        result.constraints = serde_json::json!({"operator_constraints":result.constraints});
    }
    result
        .constraints
        .as_object_mut()
        .unwrap()
        .insert(CONSTRAINT.into(), serde_json::json!(version));
    result
}
fn invalid(message: &str) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}

pub(super) fn request_recovery(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    request: &ReasoningRequestV2,
    inputs: &[InputDataSelection],
) -> Result<Option<(VersionId, ReasoningRequestV2)>, ReasoningError> {
    let Some(value) = request.constraints.get(CONSTRAINT) else {
        return Ok(None);
    };
    let version: VersionId = serde_json::from_value(value.clone())?;
    let (definition, step) = validation::load(registry, schemas, store, &version)?;
    if self::request(&definition, &step, &version) != *request || definition.inputs != inputs {
        return Err(invalid("request differs from its authorized recovery step"));
    }
    if InvocationService::new(store, schemas, &SystemClock)
        .lookup(&invocation_id(request))?
        .is_some_and(|state| state.status == InvocationStatus::Failed)
    {
        return Err(invalid(
            "completed recovery attempt cannot be dispatched again",
        ));
    }
    Ok(Some((version, definition.base_request)))
}

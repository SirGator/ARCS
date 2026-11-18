//! Durable choice of a permitted Reason endpoint, with no execution authority.
mod records;
#[cfg(test)]
mod tests;

use super::{ReasoningError, ReasoningMethod, ReasoningRequestV2, ReasoningResources};
use crate::adapters::{
    AdapterRegistry, AdapterRole, CapabilityQuery, CapabilityRef, ProducerClass,
};
use crate::core::{Artifact, ArtifactIdGenerator, Clock, SchemaId, SchemaRegistry, VersionId};
use crate::store::SqliteArtifactStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

pub const SCHEMA: &str = "arcs.reasoner_selection.v1";
pub const PUBLISHER: &str = "arcs.reasoning.selection";
pub const CONSTRAINT: &str = "arcs_reasoner_selection";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonerChoice {
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub budget: ReasoningResources,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionOwner {
    pub definition_version: VersionId,
    pub input_version: VersionId,
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionPolicy {
    pub target_schema_id: SchemaId,
    pub query_text: String,
    pub choices: Vec<ReasonerChoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonerAvailability {
    Ready,
    Busy,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonerObservation {
    pub availability: ReasonerAvailability,
    pub matched: bool,
    pub relevance_score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionRecord {
    pub owner: SelectionOwner,
    pub policy: SelectionPolicy,
    pub observations: Vec<ReasonerObservation>,
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub struct SelectedReasoner {
    pub version_id: VersionId,
    pub record: SelectionRecord,
}
impl SelectedReasoner {
    pub fn choice(&self) -> &ReasonerChoice {
        &self.record.policy.choices[self.record.selected_index]
    }
}

#[derive(Debug)]
pub enum SelectionPreparation {
    Selected(Box<SelectedReasoner>),
    Waiting,
}

fn invalid(message: impl Into<String>) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}

impl SelectionPolicy {
    pub fn validate(
        &self,
        registry: &AdapterRegistry,
        schemas: &SchemaRegistry,
    ) -> Result<(), ReasoningError> {
        if self.choices.is_empty() || self.choices.len() > 8 {
            return Err(invalid("selection needs 1..8 reasoner choices"));
        }
        let mut seen = HashSet::new();
        let schema = schemas.get(&self.target_schema_id).ok_or_else(|| {
            ReasoningError::MissingRegisteredSchema(self.target_schema_id.clone())
        })?;
        if schema.artifact_type != "candidate" && !schema.artifact_type.ends_with("_candidate") {
            return Err(ReasoningError::ReasoningOutputMustBeCandidate(
                self.target_schema_id.clone(),
            ));
        }
        for choice in &self.choices {
            if !seen.insert((&choice.capability, choice.method)) {
                return Err(invalid("duplicate reasoner choice"));
            }
            let (adapter, descriptor) = registry.authorized_capability(
                &choice.capability.adapter_id,
                &choice.capability.capability_id,
            )?;
            let limits = adapter
                .grant()
                .reasoning_limits_v2
                .as_ref()
                .ok_or(ReasoningError::ReasoningBudgetExceedsGrant)?;
            if !descriptor.contract.is_reasoning()
                || !descriptor
                    .contract
                    .emitted_schemas()
                    .contains(&self.target_schema_id)
                || !choice.budget.supports(choice.method)
                || !limits.methods.contains(&choice.method)
                || !choice.budget.fits_within(&limits.resources)
                || adapter.grant().producer_class == ProducerClass::Executor
                || choice.method.uses_model()
                    && adapter.grant().producer_class != ProducerClass::Model
            {
                return Err(invalid(
                    "reasoner choice exceeds its current contract or grant",
                ));
            }
        }
        self.query()
            .validate()
            .map_err(|error| invalid(format!("invalid selection query: {error:?}")))?;
        Ok(())
    }
    fn query(&self) -> CapabilityQuery {
        CapabilityQuery {
            text: self.query_text.clone(),
            role: Some(AdapterRole::Reason),
            accepts_schema: None,
            emits_schema: Some(self.target_schema_id.clone()),
            reasoning_method: None,
            allowed_capabilities: Some(
                self.choices
                    .iter()
                    .map(|choice| choice.capability.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            ),
            limit: 8,
        }
    }
}

pub struct ReasonerSelectionService<'a> {
    pub registry: &'a AdapterRegistry,
    pub schemas: &'a SchemaRegistry,
    pub store: &'a SqliteArtifactStore,
    pub ids: &'a mut dyn ArtifactIdGenerator,
    pub clock: &'a dyn Clock,
}

impl ReasonerSelectionService<'_> {
    pub fn load(
        &self,
        owner: &SelectionOwner,
        policy: &SelectionPolicy,
    ) -> Result<Option<SelectedReasoner>, ReasoningError> {
        load_selection(self.registry, self.schemas, self.store, owner, policy)
    }

    pub fn select(
        &mut self,
        owner: &SelectionOwner,
        policy: &SelectionPolicy,
        availability: &[ReasonerAvailability],
    ) -> Result<SelectionPreparation, ReasoningError> {
        if let Some(saved) = self.load(owner, policy)? {
            return Ok(SelectionPreparation::Selected(Box::new(saved)));
        }
        if availability.len() != policy.choices.len() {
            return Err(invalid("selection availability does not match choices"));
        }
        let hits = self
            .registry
            .find_capabilities(&policy.query())
            .map_err(|error| invalid(format!("selection discovery failed: {error:?}")))?;
        let observations: Vec<_> = policy
            .choices
            .iter()
            .zip(availability)
            .map(|(choice, availability)| {
                let hit = hits
                    .iter()
                    .find(|hit| hit.capability.capability == choice.capability);
                ReasonerObservation {
                    availability: *availability,
                    matched: hit.is_some(),
                    relevance_score: hit.map_or(0, |hit| hit.relevance_score),
                }
            })
            .collect();
        if !observations.iter().any(|observation| observation.matched) {
            return Err(invalid(
                "no permitted reasoner matches the task selection query",
            ));
        }
        let selected = observations
            .iter()
            .enumerate()
            .filter(|(_, observation)| {
                observation.matched && observation.availability == ReasonerAvailability::Ready
            })
            .max_by(|(a, first), (b, second)| {
                first
                    .relevance_score
                    .cmp(&second.relevance_score)
                    .then_with(|| b.cmp(a))
            })
            .map(|(index, _)| index);
        let Some(selected_index) = selected else {
            return Ok(SelectionPreparation::Waiting);
        };
        let record = SelectionRecord {
            owner: owner.clone(),
            policy: policy.clone(),
            observations,
            selected_index,
        };
        let committed = records::persist(&record, self.schemas, self.store, self.ids, self.clock)?;
        Ok(SelectionPreparation::Selected(Box::new(records::read(
            &committed,
            owner,
            policy,
            self.schemas,
        )?)))
    }
}

pub fn load_selection(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    owner: &SelectionOwner,
    policy: &SelectionPolicy,
) -> Result<Option<SelectedReasoner>, ReasoningError> {
    policy.validate(registry, schemas)?;
    records::validate_owner(store, owner)?;
    store
        .find_by_stream_key(&records::stream(owner))?
        .map(|artifact| records::read(&artifact, owner, policy, schemas))
        .transpose()
}

/// Existence is only a lookup hint. A returned receipt must still pass
/// load_selection with the expected policy before it may be used.
pub fn selection_exists(
    store: &SqliteArtifactStore,
    owner: &SelectionOwner,
) -> Result<bool, ReasoningError> {
    records::validate_owner(store, owner)?;
    Ok(store.find_by_stream_key(&records::stream(owner))?.is_some())
}

/// Core validation of the selection referenced by a concrete request. The
/// receipt is linked as provenance, without disclosing its private query fields
/// as an additional external context item or increasing the caller's budget.
pub(super) fn request_selection(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    request: &ReasoningRequestV2,
) -> Result<Option<VersionId>, ReasoningError> {
    let Some(value) = request.constraints.get(CONSTRAINT) else {
        return Ok(None);
    };
    let version: VersionId = serde_json::from_value(value.clone())?;
    let artifact = store
        .get(&version)?
        .ok_or_else(|| invalid("missing reasoner selection receipt"))?;
    let record: SelectionRecord = serde_json::from_value(artifact.payload.clone())?;
    record.policy.validate(registry, schemas)?;
    records::validate_owner(store, &record.owner)?;
    let selection = records::read(&artifact, &record.owner, &record.policy, schemas)?;
    let choice = selection.choice();
    if choice.capability != request.capability
        || choice.method != request.method
        || choice.budget != request.budget
        || record.policy.target_schema_id != request.target_schema_id
        || !request
            .context
            .iter()
            .any(|item| item.version_id == record.owner.input_version)
    {
        return Err(invalid(
            "request does not match its frozen reasoner selection",
        ));
    }
    Ok(Some(version))
}

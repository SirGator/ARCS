//! Automatische Kontextauswahl aus demselben Artefaktbestand wie das Reasoning.
//! Auswahl wird als Core-Artefakt eingefroren; sie erteilt keine Befugnisse.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::service::select_payload_fields;
use super::{
    ContextSelection, ReasoningContextItem, ReasoningError, ReasoningInvocationV2,
    ReasoningRequestV2,
};
use crate::adapters::{
    AdapterRegistry,
    input_data::{InputDataSelection, InputDataService},
};
use crate::core::{
    Actor, ActorType, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId, validate_artifact,
};
use crate::runtime::{InvocationKind, deterministic_invocation_id};
use crate::store::retrieval::{
    ArtifactFilter, ArtifactRetrieval, RetrievalLimits, RetrievalQuery, RetrievalTrace,
};
use crate::store::{ArtifactRelation, RelationKind, SqliteArtifactStore, relation_kinds};

mod text;
pub use text::MemoryTextQuery;

const SCHEMA: &str = "arcs.context_selection.v1";
const PRODUCER: &str = "arcs.context_selection";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryScope {
    pub filter: ArtifactFilter,
    pub payload_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_query: Option<MemoryTextQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<VersionId>,
    pub scopes: Vec<MemoryScope>,
    pub relations: Vec<RelationKind>,
    pub search: RetrievalLimits,
    /// Zusätzliche Kontextstücke; Pflichtkontext wird niemals verdrängt.
    pub max_items: usize,
    /// Serialisierte zusätzliche ReasoningContextItems, einschließlich Klammern.
    pub max_bytes: usize,
}
impl MemoryPolicy {
    pub fn validate(&self, schemas: &SchemaRegistry) -> Result<(), ReasoningError> {
        let mut anchors = HashSet::new();
        let mut relations = HashSet::new();
        if !self.search.valid()
            || self.anchors.len() > 32
            || self.scopes.is_empty()
            || self.scopes.len() > 16
            || self.relations.len() > 32
            || !(1..=64).contains(&self.max_items)
            || !(128..=1_048_576).contains(&self.max_bytes)
            || self
                .anchors
                .iter()
                .any(|id| id.0.is_empty() || id.0.len() > 512 || !anchors.insert(id))
            || self
                .relations
                .iter()
                .any(|kind| RelationKind::new(kind.as_str()).is_err() || !relations.insert(kind))
        {
            return Err(invalid("invalid memory policy limits or identities"));
        }
        for scope in &self.scopes {
            let mut fields = HashSet::new();
            if scope
                .text_query
                .as_ref()
                .is_some_and(|query| !query.valid())
                || schemas.get(&scope.filter.schema_id).is_none()
                || scope.payload_fields.is_empty()
                || scope.payload_fields.len() > 64
                || scope.payload_fields.iter().any(|field| {
                    field.trim().is_empty()
                        || field.len() > 256
                        || field.chars().any(char::is_control)
                        || !fields.insert(field)
                })
                || scope.filter.tags.len() > 32
                || scope.filter.tags.iter().any(|tag| {
                    tag.is_empty() || tag.len() > 1024 || tag.chars().any(char::is_control)
                })
                || scope.filter.subject.as_ref().is_some_and(|subject| {
                    subject.0.is_empty()
                        || subject.0.len() > 512
                        || subject.0.chars().any(char::is_control)
                })
            {
                return Err(invalid("invalid memory scope or payload fields"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedMemory {
    selection: ContextSelection,
    distance: usize,
    path: Vec<ArtifactRelation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRecord {
    request: ReasoningRequestV2,
    policy: MemoryPolicy,
    inputs: Vec<InputDataSelection>,
    selected: Vec<SelectedMemory>,
    trace: RetrievalTrace,
    skipped: usize,
}

pub struct ReasoningContextService<'a> {
    pub registry: &'a AdapterRegistry,
    pub schemas: &'a SchemaRegistry,
    pub store: &'a SqliteArtifactStore,
    pub ids: &'a mut dyn ArtifactIdGenerator,
    pub clock: &'a dyn Clock,
}

fn invalid(message: &str) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}

impl ReasoningContextService<'_> {
    /// Pflichtkontext bleibt erhalten. Nur vom Betreiber erlaubte Felder
    /// passender Erinnerungen ergänzen ihn innerhalb beider Größenbudgets.
    pub fn augment(
        &mut self,
        request: ReasoningRequestV2,
        policy: &MemoryPolicy,
        inputs: &[InputDataSelection],
    ) -> Result<ReasoningRequestV2, ReasoningError> {
        policy.validate(self.schemas)?;
        let (registered, capability) = self.registry.authorized_capability(
            &request.capability.adapter_id,
            &request.capability.capability_id,
        )?;
        let limits = registered
            .grant()
            .reasoning_limits_v2
            .as_ref()
            .ok_or(ReasoningError::ReasoningBudgetExceedsGrant)?;
        if !capability.contract.is_reasoning()
            || !limits.methods.contains(&request.method)
            || !request.budget.supports(request.method)
            || !request.budget.fits_within(&limits.resources)
        {
            return Err(ReasoningError::ReasoningBudgetExceedsGrant);
        }
        if request.context.len() > request.budget.max_context_items {
            return Err(invalid("mandatory context exceeds item budget"));
        }
        self.check_wire_budget(&request, inputs)?;
        let key = deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                "context-v1",
                &request.capability.adapter_id.0,
                &request.capability.capability_id.0,
                &request.request_id,
            ],
        );
        let stream = format!("context-selection:{key}");
        if let Some(saved) = self.store.find_by_stream_key(&stream)? {
            validate_artifact(&saved, self.schemas)
                .map_err(|_| invalid("invalid saved memory selection"))?;
            if saved.schema_id.0 != SCHEMA
                || saved.created_by.id != PRODUCER
                || saved.created_by.actor_type != ActorType::System
                || saved.source.kind != SourceKind::Internal
                || saved.trust.source_class != SourceClass::System
            {
                return Err(invalid("saved memory selection has another producer"));
            }
            let record: ContextRecord = serde_json::from_value(saved.payload)?;
            if record.request != request || record.policy != *policy || record.inputs != inputs {
                return Err(invalid("memory selection identity conflict"));
            }
            let result = self.apply_record(request, &record)?;
            self.check_wire_budget(&result, inputs)?;
            return Ok(result);
        }
        let mut seeds: Vec<_> = request
            .context
            .iter()
            .map(|selection| selection.version_id.clone())
            .collect();
        for anchor in &policy.anchors {
            if !seeds.contains(anchor) {
                seeds.push(anchor.clone());
            }
        }
        let scopes = text::resolve_scopes(self.store, &request, policy)?;
        let result = ArtifactRetrieval::new(self.store).recall(&RetrievalQuery {
            seeds,
            scopes: scopes.clone(),
            relations: policy.relations.clone(),
            limits: policy.search.clone(),
        })?;
        let mut record = ContextRecord {
            request: request.clone(),
            policy: policy.clone(),
            inputs: inputs.to_vec(),
            selected: vec![],
            trace: result.trace,
            skipped: 0,
        };
        let mut augmented = request.clone();
        let mut memory_items = Vec::new();
        let mut seen: HashSet<_> = request
            .context
            .iter()
            .map(|selection| selection.version_id.clone())
            .collect();
        for hit in result.hits {
            if !seen.insert(hit.artifact.version_id.clone()) {
                continue;
            }
            if record.selected.len() >= policy.max_items
                || augmented.context.len() >= request.budget.max_context_items
            {
                record.skipped += 1;
                continue;
            }
            let scope = policy
                .scopes
                .iter()
                .zip(&scopes)
                .find(|(_, scope)| scope.matches(&hit.artifact))
                .map(|(scope, _)| scope)
                .expect("retrieval respects filters");
            let selection = ContextSelection {
                version_id: hit.artifact.version_id.clone(),
                payload_fields: scope.payload_fields.clone(),
            };
            let item = match self.project(&selection) {
                Ok(item) => item,
                Err(
                    ReasoningError::InvalidContextField { .. }
                    | ReasoningError::ContextPayloadMustBeObject(_),
                ) => {
                    record.skipped += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            memory_items.push(item);
            if serde_json::to_vec(&memory_items)?.len() > policy.max_bytes {
                memory_items.pop();
                record.skipped += 1;
                continue;
            }
            augmented.context.push(selection.clone());
            match self.check_wire_budget(&augmented, inputs) {
                Ok(()) => {}
                Err(ReasoningError::ContextTooLarge { .. }) => {
                    augmented.context.pop();
                    memory_items.pop();
                    record.skipped += 1;
                    continue;
                }
                Err(error) => return Err(error),
            }
            record.selected.push(SelectedMemory {
                selection,
                distance: hit.distance,
                path: hit.path,
            });
        }
        let parents: Vec<_> = augmented
            .context
            .iter()
            .map(|selection| selection.version_id.clone())
            .collect();
        let artifact = ArtifactFactory::new(self.clock, self.ids)
            .create(ArtifactFactoryInput {
                schema: self
                    .schemas
                    .get(&SchemaId(SCHEMA.into()))
                    .ok_or_else(|| {
                        ReasoningError::MissingRegisteredSchema(SchemaId(SCHEMA.into()))
                    })?
                    .clone(),
                created_by: Actor {
                    actor_type: ActorType::System,
                    id: PRODUCER.into(),
                },
                source: Source {
                    kind: SourceKind::Internal,
                    reference: stream.clone(),
                },
                trust: Trust {
                    level: TrustLevel::High,
                    source_class: SourceClass::System,
                },
                stream_key: stream,
                subject: SubjectId(key),
                tags: vec![],
                payload: serde_json::to_value(&record)?,
                provenance: Some(Provenance {
                    parents: parents.iter().map(|id| id.0.clone()).collect(),
                    rules_applied: vec!["context.memory.v1".into()],
                    ..Default::default()
                }),
            })
            .map_err(|_| invalid("cannot construct memory selection artifact"))?;
        let relations: Vec<_> = parents
            .into_iter()
            .map(|id| (id, relation_kinds::based_on()))
            .collect();
        let identity = serde_json::to_string(&(&request, policy, inputs))?;
        let committed = self.store.append_current_related_once(
            &artifact,
            self.schemas,
            crate::store::database::Idempotency {
                key: &artifact.stream_key,
                input: &identity,
            },
            &relations,
        )?;
        let persisted: ContextRecord = serde_json::from_value(committed.artifact.payload)?;
        let augmented = self.apply_record(request, &persisted)?;
        self.check_wire_budget(&augmented, inputs)?;
        Ok(augmented)
    }

    fn apply_record(
        &self,
        mut request: ReasoningRequestV2,
        record: &ContextRecord,
    ) -> Result<ReasoningRequestV2, ReasoningError> {
        if record.selected.len() > record.policy.max_items {
            return Err(invalid("saved memory item limit exceeded"));
        }
        let mut seen: HashSet<_> = request
            .context
            .iter()
            .map(|item| item.version_id.clone())
            .collect();
        let mut items = Vec::new();
        let scopes = text::resolve_scopes(self.store, &request, &record.policy)?;
        for selected in &record.selected {
            let artifact = self
                .store
                .get(&selected.selection.version_id)?
                .ok_or_else(|| {
                    ReasoningError::MissingContextArtifact(selected.selection.version_id.clone())
                })?;
            if !seen.insert(selected.selection.version_id.clone())
                || !record
                    .policy
                    .scopes
                    .iter()
                    .zip(&scopes)
                    .any(|(scope, resolved)| {
                        resolved.matches(&artifact)
                            && scope.payload_fields == selected.selection.payload_fields
                    })
            {
                return Err(invalid("saved memory selection exceeds configured scope"));
            }
            items.push(self.project(&selected.selection)?);
            request.context.push(selected.selection.clone());
        }
        if request.context.len() > request.budget.max_context_items
            || serde_json::to_vec(&items)?.len() > record.policy.max_bytes
        {
            return Err(invalid("saved memory context exceeds budget"));
        }
        Ok(request)
    }

    fn project(
        &self,
        selection: &ContextSelection,
    ) -> Result<ReasoningContextItem, ReasoningError> {
        let artifact = self
            .store
            .get(&selection.version_id)?
            .ok_or_else(|| ReasoningError::MissingContextArtifact(selection.version_id.clone()))?;
        validate_artifact(&artifact, self.schemas)
            .map_err(|_| invalid("invalid stored memory artifact"))?;
        Ok(ReasoningContextItem {
            payload: select_payload_fields(&artifact, &selection.payload_fields)?,
            version_id: artifact.version_id,
            schema_id: artifact.schema_id,
            artifact_type: artifact.artifact_type,
            trust_level: artifact.trust.level,
        })
    }

    fn check_wire_budget(
        &self,
        request: &ReasoningRequestV2,
        inputs: &[InputDataSelection],
    ) -> Result<(), ReasoningError> {
        let mut seen = HashSet::new();
        let mut context = Vec::new();
        for selection in &request.context {
            if !seen.insert(&selection.version_id) {
                return Err(ReasoningError::DuplicateContextArtifact(
                    selection.version_id.clone(),
                ));
            }
            context.push(self.project(selection)?);
        }
        context.sort_by(|a, b| a.version_id.0.cmp(&b.version_id.0));
        let input_data = InputDataService::new(self.registry, self.schemas, self.store).prepare(
            &request.capability,
            &context,
            inputs,
        )?;
        let invocation = ReasoningInvocationV2 {
            invocation_id: deterministic_invocation_id(
                InvocationKind::Reasoning,
                &[
                    "v2",
                    &request.capability.adapter_id.0,
                    &request.capability.capability_id.0,
                    &request.request_id,
                ],
            ),
            request_id: request.request_id.clone(),
            capability: request.capability.clone(),
            method: request.method,
            objective: request.objective.clone(),
            context,
            input_data,
            target_schema_id: request.target_schema_id.clone(),
            allowed_capabilities: request.allowed_capabilities.clone(),
            constraints: request.constraints.clone(),
            budget: request.budget.clone(),
        };
        let bytes = serde_json::to_vec(&invocation)?.len();
        if bytes > request.budget.max_context_bytes {
            return Err(ReasoningError::ContextTooLarge {
                actual: bytes,
                maximum: request.budget.max_context_bytes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

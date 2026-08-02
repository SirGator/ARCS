//! Use Case für minimiertes, budgetiertes und nicht autorisierendes Reasoning.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::ReasoningError;
use crate::adapters::registration::{AdapterRegistry, CapabilityRef, ProducerClass};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactIdGenerator, Clock, MAX_MODEL_TRACE_TEXT_BYTES, Provenance,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::reasoning::{
    ReasoningAdapter, ReasoningContextItem, ReasoningInvocation, ReasoningRequest,
    ValidatedProposal,
};
use crate::store::SqliteArtifactStore;

const REASONING_REQUEST_SCHEMA_ID: &str = "arcs.reasoning_request.v1";

/// Eigenständiger Reasoning-Slice mit genau einem externen Modellport.
pub struct ReasoningService<'a> {
    pub(super) policy: &'a AdapterRegistry,
    pub(super) schemas: &'a SchemaRegistry,
    pub(super) store: &'a SqliteArtifactStore,
    pub(super) ids: &'a mut dyn ArtifactIdGenerator,
    pub(super) clock: &'a dyn Clock,
    pub(super) endpoint: &'a dyn ReasoningAdapter,
    pub(super) used_reasoning_requests: HashSet<(CapabilityRef, String)>,
    pub(super) committed_proposals: HashSet<(VersionId, usize)>,
}

impl<'a> ReasoningService<'a> {
    pub fn new(
        policy: &'a AdapterRegistry,
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
        endpoint: &'a dyn ReasoningAdapter,
    ) -> Self {
        Self {
            policy,
            schemas,
            store,
            ids,
            clock,
            endpoint,
            used_reasoning_requests: HashSet::new(),
            committed_proposals: HashSet::new(),
        }
    }

    pub(crate) fn schemas(&self) -> &SchemaRegistry {
        self.schemas
    }
}

impl ReasoningService<'_> {
    /// Ruft einen ReasoningAdapter ausschließlich mit explizit ausgewähltem,
    /// minimiertem Kontext auf und validiert sämtliche Vorschläge.
    ///
    /// Vor dem externen Aufruf wird genau der kuratierte Auftrag als
    /// `ReasoningRequest`-Artifact gespeichert. Ein gültiges Modellergebnis
    /// bleibt trotzdem ein `ValidatedProposal` ohne Execution-Autorität.
    pub fn reason(
        &mut self,
        request: ReasoningRequest,
    ) -> Result<Vec<ValidatedProposal>, ReasoningError> {
        validate_reasoning_budget_and_request(&request)?;

        let adapter_id = request.reasoning_capability.adapter_id.clone();
        if self.endpoint.manifest().adapter_id != adapter_id {
            return Err(ReasoningError::MissingReasoningEndpoint(adapter_id));
        }
        let (authorized_capability, emitted_schemas, reasoning_limits) = {
            let (registered, capability) = self
                .policy
                .authorized_capability(&adapter_id, &request.reasoning_capability.capability_id)?;
            if !capability.contract.is_reasoning() {
                return Err(ReasoningError::NotReasoningAdapter(adapter_id.clone()));
            }
            if registered.grant().producer_class != ProducerClass::Model {
                return Err(ReasoningError::ReasoningProducerMustBeModel(
                    adapter_id.clone(),
                ));
            }
            (
                CapabilityRef {
                    adapter_id: registered.manifest().adapter_id.clone(),
                    capability_id: capability.id.clone(),
                },
                capability.contract.emitted_schemas().to_vec(),
                registered
                    .grant()
                    .reasoning_limits
                    .clone()
                    .ok_or(ReasoningError::ReasoningBudgetExceedsGrant)?,
            )
        };
        if !request.budget.fits_within(&reasoning_limits) {
            return Err(ReasoningError::ReasoningBudgetExceedsGrant);
        }
        if !emitted_schemas.contains(&request.target_schema_id) {
            return Err(ReasoningError::UndeclaredOutputSchema {
                capability: request.reasoning_capability.capability_id.clone(),
                schema: request.target_schema_id.clone(),
            });
        }
        ensure_candidate_schema(self.schemas, &request.target_schema_id)?;

        let allowed_capabilities =
            validate_allowed_capabilities(self.policy, &request.allowed_capabilities)?;
        let context = self.build_reasoning_context(&request)?;
        let context_versions = context
            .iter()
            .map(|item| item.version_id.clone())
            .collect::<Vec<_>>();
        let context_set = context_versions.iter().cloned().collect::<HashSet<_>>();

        let invocation = ReasoningInvocation {
            request_id: request.request_id.clone(),
            capability: authorized_capability.clone(),
            objective: request.objective,
            context,
            target_schema_id: request.target_schema_id.clone(),
            allowed_capabilities: allowed_capabilities.clone(),
            constraints: request.constraints,
            max_output_tokens: request.budget.max_output_tokens,
            max_candidates: request.budget.max_candidates,
        };
        let context_bytes = serde_json::to_vec(&invocation)?.len();
        if context_bytes > request.budget.max_context_bytes {
            return Err(ReasoningError::ContextTooLarge {
                actual: context_bytes,
                maximum: request.budget.max_context_bytes,
            });
        }

        // Ein fehlender Port ist ein Konfigurationsfehler und noch kein
        // tatsächlich vorbereiteter externer Reasoning-Aufruf.
        let request_key = (authorized_capability.clone(), request.request_id.clone());
        if self.used_reasoning_requests.contains(&request_key) {
            return Err(ReasoningError::ReasoningRequestAlreadyUsed {
                capability: authorized_capability.clone(),
                request_id: request.request_id.clone(),
            });
        }
        let reasoning_request_version = self.persist_reasoning_request(
            &request.request_id,
            &authorized_capability,
            &request.target_schema_id,
            &invocation.objective,
            &context_versions,
        )?;
        // Ab dem Audit-Commit bezeichnet diese ID genau einen externen
        // Reasoning-Versuch. Auch ein Transport- oder Validierungsfehler darf
        // nicht unter derselben Korrelation neu interpretiert werden.
        self.used_reasoning_requests.insert(request_key);

        let response = self.endpoint.propose(&invocation)?;

        if response.request_id != request.request_id {
            return Err(ReasoningError::ResponseRequestMismatch);
        }
        if response.candidates.len() > request.budget.max_candidates {
            return Err(ReasoningError::TooManyCandidates {
                actual: response.candidates.len(),
                maximum: request.budget.max_candidates,
            });
        }
        let response_bytes = serde_json::to_vec(&response)?.len();
        if response_bytes > request.budget.max_output_bytes {
            return Err(ReasoningError::ResponseTooLarge {
                actual: response_bytes,
                maximum: request.budget.max_output_bytes,
            });
        }
        if response.trace.model_name.trim().is_empty()
            || response.trace.model_name.len() > MAX_MODEL_TRACE_TEXT_BYTES
            || response.trace.prompt_hash.trim().is_empty()
            || response.trace.prompt_hash.len() > MAX_MODEL_TRACE_TEXT_BYTES
            || response.trace.raw_output_hash.trim().is_empty()
            || response.trace.raw_output_hash.len() > MAX_MODEL_TRACE_TEXT_BYTES
            || response.trace.model_name.chars().any(char::is_control)
            || response.trace.prompt_hash.chars().any(char::is_control)
            || response.trace.raw_output_hash.chars().any(char::is_control)
            || !response.trace.temperature.is_finite()
            || response.trace.temperature < 0.0
        {
            return Err(ReasoningError::InvalidReasoningTrace);
        }

        let allowed_set = allowed_capabilities.into_iter().collect::<HashSet<_>>();
        let mut proposals = Vec::with_capacity(response.candidates.len());
        for (candidate_index, candidate) in response.candidates.into_iter().enumerate() {
            if candidate.schema_id != request.target_schema_id {
                return Err(ReasoningError::UnexpectedCandidateSchema(
                    candidate.schema_id,
                ));
            }
            self.schemas
                .validate(&candidate.schema_id, &candidate.payload)
                .map_err(ReasoningError::InvalidPayload)?;
            ensure_candidate_schema(self.schemas, &candidate.schema_id)?;

            let required_capabilities =
                validate_candidate_capabilities(&candidate.required_capabilities, &allowed_set)?;
            let referenced_versions =
                validate_candidate_references(&candidate.referenced_versions, &context_set)?;
            proposals.push(ValidatedProposal {
                adapter_id: adapter_id.clone(),
                request_id: request.request_id.clone(),
                reasoning_request_version: reasoning_request_version.clone(),
                candidate_index,
                schema_id: candidate.schema_id,
                required_capabilities,
                referenced_versions,
                context_versions: context_versions.clone(),
                payload: candidate.payload,
                trace: response.trace.clone(),
            });
        }
        Ok(proposals)
    }

    /// Persistiert den auditierbaren Core-Auftrag. Es werden absichtlich nur
    /// Ziel und Versionsreferenzen gespeichert, niemals die ausgewählten
    /// Payloadfelder oder weitere Store-Inhalte.
    fn persist_reasoning_request(
        &mut self,
        request_id: &str,
        reasoning_capability: &CapabilityRef,
        target_schema_id: &SchemaId,
        objective: &str,
        context_versions: &[VersionId],
    ) -> Result<VersionId, ReasoningError> {
        let schema_id = SchemaId(REASONING_REQUEST_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| ReasoningError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let generated = self.ids.next(&definition.artifact_type);
        let artifact = Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.reasoning".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("reasoning-request:{request_id}"),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("reasoning:{request_id}"),
            subject: None,
            tags: vec![
                format!("adapter:{}", reasoning_capability.adapter_id.0),
                format!("capability:{}", reasoning_capability.capability_id.0),
                format!("target_schema:{}", target_schema_id.0),
            ],
            payload: json!({
                "objective": objective,
                "context_refs": context_versions
                    .iter()
                    .map(|version| version.0.clone())
                    .collect::<Vec<_>>(),
            }),
            provenance: Some(Provenance {
                parents: context_versions
                    .iter()
                    .map(|version| version.0.clone())
                    .collect(),
                rules_applied: vec![
                    "reasoning.context_minimized".into(),
                    "reasoning.budget_validated".into(),
                ],
                models_used: vec![],
                transform: Some("reasoning.prepare_request".into()),
            }),
        };

        // Der Store validiert das geschlossene Bundled Schema erneut. Erst
        // nach erfolgreichem Commit darf der externe Port aufgerufen werden.
        self.store.append(&artifact, self.schemas)?;
        Ok(artifact.version_id)
    }

    fn build_reasoning_context(
        &self,
        request: &ReasoningRequest,
    ) -> Result<Vec<ReasoningContextItem>, ReasoningError> {
        if request.context.len() > request.budget.max_context_items {
            return Err(ReasoningError::InvalidReasoningRequest(
                "context item limit exceeded".into(),
            ));
        }

        let mut seen_versions = HashSet::new();
        let mut context = Vec::with_capacity(request.context.len());
        for selection in &request.context {
            if !seen_versions.insert(selection.version_id.clone()) {
                return Err(ReasoningError::DuplicateContextArtifact(
                    selection.version_id.clone(),
                ));
            }
            let artifact = self.store.get(&selection.version_id)?.ok_or_else(|| {
                ReasoningError::MissingContextArtifact(selection.version_id.clone())
            })?;
            let payload = select_payload_fields(&artifact, &selection.payload_fields)?;
            context.push(ReasoningContextItem {
                version_id: artifact.version_id,
                schema_id: artifact.schema_id,
                artifact_type: artifact.artifact_type,
                trust_level: artifact.trust.level,
                payload,
            });
        }

        // Stabile Reihenfolge erzeugt reproduzierbare Prompts und Hashes.
        context.sort_by(|left, right| left.version_id.0.cmp(&right.version_id.0));
        Ok(context)
    }
}

fn validate_reasoning_budget_and_request(request: &ReasoningRequest) -> Result<(), ReasoningError> {
    if request.request_id.trim().is_empty()
        || request.request_id.len() > 512
        || request.request_id.chars().any(char::is_control)
        || request.objective.trim().is_empty()
    {
        return Err(ReasoningError::InvalidReasoningRequest(
            "request_id must be non-empty, control-free, and at most 512 bytes; objective must be non-empty"
                .into(),
        ));
    }
    if request.budget.max_context_items == 0
        || request.budget.max_context_bytes == 0
        || request.budget.max_output_tokens == 0
        || request.budget.max_output_bytes == 0
        || request.budget.max_candidates == 0
    {
        return Err(ReasoningError::InvalidReasoningRequest(
            "all reasoning budget limits must be positive".into(),
        ));
    }
    Ok(())
}

pub(super) fn ensure_candidate_schema(
    schemas: &SchemaRegistry,
    schema_id: &SchemaId,
) -> Result<(), ReasoningError> {
    let definition = schemas
        .get(schema_id)
        .ok_or_else(|| ReasoningError::MissingRegisteredSchema(schema_id.clone()))?;
    if definition.artifact_type != "candidate" && !definition.artifact_type.ends_with("_candidate")
    {
        return Err(ReasoningError::ReasoningOutputMustBeCandidate(
            schema_id.clone(),
        ));
    }
    Ok(())
}

fn validate_allowed_capabilities(
    registry: &AdapterRegistry,
    capabilities: &[CapabilityRef],
) -> Result<Vec<CapabilityRef>, ReasoningError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !seen.insert(capability.clone()) || !registry.is_enabled_capability(capability) {
            return Err(ReasoningError::UnknownAllowedCapability(capability.clone()));
        }
        validated.push(capability.clone());
    }
    validated.sort();
    Ok(validated)
}

fn validate_candidate_capabilities(
    capabilities: &[CapabilityRef],
    allowed: &HashSet<CapabilityRef>,
) -> Result<Vec<CapabilityRef>, ReasoningError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !seen.insert(capability.clone()) || !allowed.contains(capability) {
            return Err(ReasoningError::ForbiddenCandidateCapability(
                capability.clone(),
            ));
        }
        validated.push(capability.clone());
    }
    validated.sort();
    Ok(validated)
}

fn validate_candidate_references(
    versions: &[VersionId],
    context: &HashSet<VersionId>,
) -> Result<Vec<VersionId>, ReasoningError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(versions.len());
    for version in versions {
        if !seen.insert(version.clone()) || !context.contains(version) {
            return Err(ReasoningError::CandidateReferenceOutsideContext(
                version.clone(),
            ));
        }
        validated.push(version.clone());
    }
    validated.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(validated)
}

fn select_payload_fields(artifact: &Artifact, fields: &[String]) -> Result<Value, ReasoningError> {
    let payload = artifact
        .payload
        .as_object()
        .ok_or_else(|| ReasoningError::ContextPayloadMustBeObject(artifact.version_id.clone()))?;
    let mut selected = Map::new();
    let mut seen = HashSet::new();
    for field in fields {
        if field.trim().is_empty() || !seen.insert(field) {
            return Err(ReasoningError::InvalidContextField {
                version: artifact.version_id.clone(),
                field: field.clone(),
            });
        }
        let value = payload
            .get(field)
            .ok_or_else(|| ReasoningError::InvalidContextField {
                version: artifact.version_id.clone(),
                field: field.clone(),
            })?;
        selected.insert(field.clone(), value.clone());
    }
    Ok(Value::Object(selected))
}

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
    ReasoningResponse, ValidatedProposal,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_input_fingerprint, deterministic_invocation_id,
};
use crate::store::SqliteArtifactStore;

const REASONING_REQUEST_SCHEMA_ID: &str = "arcs.reasoning_request.v1";
const REASONING_RESULT_SCHEMA_ID: &str = "arcs.reasoning_result.v1";

/// Eigenständiger Reasoning-Slice mit genau einem externen Modellport.
pub struct ReasoningService<'a> {
    pub(super) policy: &'a AdapterRegistry,
    pub(super) schemas: &'a SchemaRegistry,
    pub(super) store: &'a SqliteArtifactStore,
    pub(super) ids: &'a mut dyn ArtifactIdGenerator,
    pub(super) clock: &'a dyn Clock,
    pub(super) endpoint: &'a dyn ReasoningAdapter,
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

        let invocation_id = deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                &authorized_capability.adapter_id.0,
                &authorized_capability.capability_id.0,
                &request.request_id,
            ],
        );
        let invocation = ReasoningInvocation {
            invocation_id: invocation_id.clone(),
            request_id: request.request_id.clone(),
            capability: authorized_capability.clone(),
            objective: request.objective.clone(),
            context,
            target_schema_id: request.target_schema_id.clone(),
            allowed_capabilities: allowed_capabilities.clone(),
            constraints: request.constraints.clone(),
            max_output_tokens: request.budget.max_output_tokens,
            max_candidates: request.budget.max_candidates,
        };
        let input_fingerprint = reasoning_fingerprint(&invocation)?;
        let context_bytes = serde_json::to_vec(&invocation)?.len();
        if context_bytes > request.budget.max_context_bytes {
            return Err(ReasoningError::ContextTooLarge {
                actual: context_bytes,
                maximum: request.budget.max_context_bytes,
            });
        }

        let invocations = InvocationService::new(self.store, self.schemas, self.clock);
        let existing = invocations.lookup(&invocation_id)?;
        if let Some(existing) = &existing {
            let spec = InvocationSpec {
                invocation_id: invocation_id.clone(),
                kind: InvocationKind::Reasoning,
                capability: capability_name(&authorized_capability),
                input_version: existing.input_version.clone(),
                input_fingerprint: input_fingerprint.clone(),
            };
            invocations.assert_identity(existing, &spec)?;
        }
        if let Some(existing) = &existing {
            if existing.status == InvocationStatus::Succeeded {
                let result_version = existing
                    .result_version
                    .clone()
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                let result = self
                    .store
                    .get(&result_version)?
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                let response: ReasoningResponse = serde_json::from_value(
                    result.payload.get("response").cloned().ok_or_else(|| {
                        ReasoningError::InvalidReasoningRequest("stored result is malformed".into())
                    })?,
                )?;
                return self.validate_response(
                    response,
                    &request,
                    &authorized_capability,
                    &context_versions,
                    &allowed_capabilities,
                    &existing.input_version,
                    &invocation_id,
                );
            }
        }

        let audit = if existing.is_none() {
            Some(self.build_reasoning_request_artifact(
                &request.request_id,
                &authorized_capability,
                &request.target_schema_id,
                &invocation.objective,
                &context_versions,
            )?)
        } else {
            None
        };
        let (prepared, reasoning_request_version) = if let Some(existing) = existing {
            let version = existing.input_version.clone();
            (existing, version)
        } else {
            let audit = audit.ok_or_else(|| {
                ReasoningError::InvalidReasoningRequest(
                    "missing audit artifact for new invocation".into(),
                )
            })?;
            let version = audit.version_id.clone();
            let prepared = InvocationService::new(self.store, self.schemas, self.clock)
                .prepare_with_event(
                    InvocationSpec {
                        invocation_id: invocation_id.clone(),
                        kind: InvocationKind::Reasoning,
                        capability: capability_name(&authorized_capability),
                        input_version: version.clone(),
                        input_fingerprint: input_fingerprint.clone(),
                    },
                    &audit,
                )?;
            (prepared, version)
        };
        let dispatched = {
            let invocations = InvocationService::new(self.store, self.schemas, self.clock);
            let recovered = invocations.recover(&prepared)?;
            invocations.dispatch(&recovered)?
        };
        let outcome = (|| {
            let response = self.endpoint.propose(&invocation)?;
            let proposals = self.validate_response(
                response.clone(),
                &request,
                &authorized_capability,
                &context_versions,
                &allowed_capabilities,
                &reasoning_request_version,
                &invocation_id,
            )?;
            let result = self.build_reasoning_result_artifact(&invocation_id, response)?;
            InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                &dispatched,
                &result,
                &[],
            )?;
            Ok(proposals)
        })();
        if let Err(error) = &outcome {
            let _ = InvocationService::new(self.store, self.schemas, self.clock).fail(
                &dispatched,
                format!("{error:?}"),
                is_retryable(error),
            );
        }
        outcome
    }

    fn validate_response(
        &self,
        response: ReasoningResponse,
        request: &ReasoningRequest,
        authorized_capability: &CapabilityRef,
        context_versions: &[VersionId],
        allowed_capabilities: &[CapabilityRef],
        reasoning_request_version: &VersionId,
        invocation_id: &str,
    ) -> Result<Vec<ValidatedProposal>, ReasoningError> {
        if response.invocation_id != invocation_id {
            return Err(ReasoningError::InvocationResponseMismatch);
        }
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

        let context_set = context_versions.iter().cloned().collect::<HashSet<_>>();
        let allowed_set = allowed_capabilities.iter().cloned().collect::<HashSet<_>>();
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
                adapter_id: authorized_capability.adapter_id.clone(),
                reasoning_capability: authorized_capability.clone(),
                request_id: request.request_id.clone(),
                reasoning_request_version: reasoning_request_version.clone(),
                candidate_index,
                schema_id: candidate.schema_id,
                required_capabilities,
                referenced_versions,
                context_versions: context_versions.to_vec(),
                payload: candidate.payload,
                trace: response.trace.clone(),
            });
        }
        Ok(proposals)
    }

    /// Baut den auditierbaren Core-Auftrag. Der Invocation-Service speichert
    /// ihn zusammen mit `prepared` atomar, bevor ein Modellport aufgerufen wird.
    fn build_reasoning_request_artifact(
        &mut self,
        request_id: &str,
        reasoning_capability: &CapabilityRef,
        target_schema_id: &SchemaId,
        objective: &str,
        context_versions: &[VersionId],
    ) -> Result<Artifact, ReasoningError> {
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

        Ok(artifact)
    }

    fn build_reasoning_result_artifact(
        &mut self,
        invocation_id: &str,
        response: ReasoningResponse,
    ) -> Result<Artifact, ReasoningError> {
        let schema_id = SchemaId(REASONING_RESULT_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| ReasoningError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let generated = self.ids.next(&definition.artifact_type);
        Ok(Artifact {
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
                reference: format!("reasoning-result:{invocation_id}"),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("reasoning:{invocation_id}"),
            subject: None,
            tags: vec![format!("invocation:{invocation_id}")],
            payload: json!({"response": response}),
            provenance: Some(Provenance {
                parents: vec![],
                rules_applied: vec!["reasoning.response_validated".into()],
                models_used: vec![],
                transform: Some("reasoning.persist_result".into()),
            }),
        })
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

fn reasoning_fingerprint(invocation: &ReasoningInvocation) -> Result<String, ReasoningError> {
    let serialized = serde_json::to_string(invocation)?;
    Ok(deterministic_input_fingerprint(&[&serialized]))
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

fn is_retryable(error: &ReasoningError) -> bool {
    matches!(
        error,
        ReasoningError::AdapterCall(
            crate::adapters::AdapterCallError::Unavailable(_)
                | crate::adapters::AdapterCallError::Timeout
        )
    )
}

fn capability_name(capability: &CapabilityRef) -> String {
    format!("{}/{}", capability.adapter_id.0, capability.capability_id.0)
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

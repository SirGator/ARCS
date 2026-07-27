//! Use Case für minimiertes, budgetiertes und nicht autorisierendes Reasoning.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::{AdapterGateway, AdapterGatewayError};
use crate::adapters::reasoning::{
    ReasoningContextItem, ReasoningInvocation, ReasoningRequest, ValidatedProposal,
};
use crate::adapters::registration::{
    AdapterGrant, AdapterManifest, AdapterRegistry, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, MAX_MODEL_TRACE_TEXT_BYTES, Provenance, SchemaId, SchemaRegistry,
    Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};

const REASONING_REQUEST_SCHEMA_ID: &str = "arcs.reasoning_request.v1";

impl AdapterGateway<'_> {
    /// Ruft einen ReasoningAdapter ausschließlich mit explizit ausgewähltem,
    /// minimiertem Kontext auf und validiert sämtliche Vorschläge.
    ///
    /// Vor dem externen Aufruf wird genau der kuratierte Auftrag als
    /// `ReasoningRequest`-Artifact gespeichert. Ein gültiges Modellergebnis
    /// bleibt trotzdem ein `ValidatedProposal` ohne Execution-Autorität.
    pub(crate) fn reason(
        &mut self,
        request: ReasoningRequest,
    ) -> Result<Vec<ValidatedProposal>, AdapterGatewayError> {
        validate_reasoning_budget_and_request(&request)?;

        let adapter_id = request.reasoning_capability.adapter_id.clone();
        let (authorized_capability, emitted_schemas, reasoning_limits) = {
            let (registered, capability) = self
                .registry
                .authorized_capability(&adapter_id, &request.reasoning_capability.capability_id)?;
            if !capability.contract.is_reasoning() {
                return Err(AdapterGatewayError::NotReasoningAdapter(adapter_id.clone()));
            }
            if registered.grant().producer_class != ProducerClass::Model {
                return Err(AdapterGatewayError::ReasoningProducerMustBeModel(
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
                    .ok_or(AdapterGatewayError::ReasoningBudgetExceedsGrant)?,
            )
        };
        if !request.budget.fits_within(&reasoning_limits) {
            return Err(AdapterGatewayError::ReasoningBudgetExceedsGrant);
        }
        if !emitted_schemas.contains(&request.target_schema_id) {
            return Err(AdapterGatewayError::UndeclaredOutputSchema {
                capability: request.reasoning_capability.capability_id.clone(),
                schema: request.target_schema_id.clone(),
            });
        }
        ensure_candidate_schema(self.schemas, &request.target_schema_id)?;

        let allowed_capabilities =
            validate_allowed_capabilities(&self.registry, &request.allowed_capabilities)?;
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
            return Err(AdapterGatewayError::ContextTooLarge {
                actual: context_bytes,
                maximum: request.budget.max_context_bytes,
            });
        }

        // Ein fehlender Port ist ein Konfigurationsfehler und noch kein
        // tatsächlich vorbereiteter externer Reasoning-Aufruf.
        if !self.reasoning_endpoints.contains_key(&adapter_id) {
            return Err(AdapterGatewayError::MissingReasoningEndpoint(
                adapter_id.clone(),
            ));
        }
        let request_key = (authorized_capability.clone(), request.request_id.clone());
        if self.used_reasoning_requests.contains(&request_key) {
            return Err(AdapterGatewayError::ReasoningRequestAlreadyUsed {
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

        let endpoint = self
            .reasoning_endpoints
            .get(&adapter_id)
            .ok_or_else(|| AdapterGatewayError::MissingReasoningEndpoint(adapter_id.clone()))?;
        let response = endpoint.propose(&invocation)?;

        if response.request_id != request.request_id {
            return Err(AdapterGatewayError::ResponseRequestMismatch);
        }
        if response.candidates.len() > request.budget.max_candidates {
            return Err(AdapterGatewayError::TooManyCandidates {
                actual: response.candidates.len(),
                maximum: request.budget.max_candidates,
            });
        }
        let response_bytes = serde_json::to_vec(&response)?.len();
        if response_bytes > request.budget.max_output_bytes {
            return Err(AdapterGatewayError::ResponseTooLarge {
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
            return Err(AdapterGatewayError::InvalidReasoningTrace);
        }

        let allowed_set = allowed_capabilities.into_iter().collect::<HashSet<_>>();
        let mut proposals = Vec::with_capacity(response.candidates.len());
        for (candidate_index, candidate) in response.candidates.into_iter().enumerate() {
            if candidate.schema_id != request.target_schema_id {
                return Err(AdapterGatewayError::UnexpectedCandidateSchema(
                    candidate.schema_id,
                ));
            }
            self.schemas
                .validate(&candidate.schema_id, &candidate.payload)
                .map_err(AdapterGatewayError::InvalidPayload)?;
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
    ) -> Result<VersionId, AdapterGatewayError> {
        let schema_id = SchemaId(REASONING_REQUEST_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| AdapterGatewayError::MissingRegisteredSchema(schema_id.clone()))?
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
                id: "arcs.reasoning_gateway".into(),
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
                    "adapter_gateway.reasoning_context_minimized".into(),
                    "adapter_gateway.reasoning_budget_validated".into(),
                ],
                models_used: vec![],
                transform: Some("adapter_gateway.prepare_reasoning_request".into()),
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
    ) -> Result<Vec<ReasoningContextItem>, AdapterGatewayError> {
        if request.context.len() > request.budget.max_context_items {
            return Err(AdapterGatewayError::InvalidReasoningRequest(
                "context item limit exceeded".into(),
            ));
        }

        let mut seen_versions = HashSet::new();
        let mut context = Vec::with_capacity(request.context.len());
        for selection in &request.context {
            if !seen_versions.insert(selection.version_id.clone()) {
                return Err(AdapterGatewayError::DuplicateContextArtifact(
                    selection.version_id.clone(),
                ));
            }
            let artifact = self.store.get(&selection.version_id)?.ok_or_else(|| {
                AdapterGatewayError::MissingContextArtifact(selection.version_id.clone())
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

fn validate_reasoning_budget_and_request(
    request: &ReasoningRequest,
) -> Result<(), AdapterGatewayError> {
    if request.request_id.trim().is_empty()
        || request.request_id.len() > 512
        || request.request_id.chars().any(char::is_control)
        || request.objective.trim().is_empty()
    {
        return Err(AdapterGatewayError::InvalidReasoningRequest(
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
        return Err(AdapterGatewayError::InvalidReasoningRequest(
            "all reasoning budget limits must be positive".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_reasoning_output_schemas(
    manifest: &AdapterManifest,
    grant: &AdapterGrant,
    schemas: &SchemaRegistry,
) -> Result<(), AdapterGatewayError> {
    for capability_id in &grant.enabled_capabilities {
        let Some(capability) = manifest.capability(capability_id) else {
            continue;
        };
        if capability.contract.is_reasoning() {
            for schema_id in capability.contract.emitted_schemas() {
                ensure_candidate_schema(schemas, schema_id)?;
            }
        }
    }
    Ok(())
}

pub(super) fn ensure_candidate_schema(
    schemas: &SchemaRegistry,
    schema_id: &SchemaId,
) -> Result<(), AdapterGatewayError> {
    let definition = schemas
        .get(schema_id)
        .ok_or_else(|| AdapterGatewayError::MissingRegisteredSchema(schema_id.clone()))?;
    if definition.artifact_type != "candidate" && !definition.artifact_type.ends_with("_candidate")
    {
        return Err(AdapterGatewayError::ReasoningOutputMustBeCandidate(
            schema_id.clone(),
        ));
    }
    Ok(())
}

fn validate_allowed_capabilities(
    registry: &AdapterRegistry,
    capabilities: &[CapabilityRef],
) -> Result<Vec<CapabilityRef>, AdapterGatewayError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !seen.insert(capability.clone()) || !registry.is_enabled_capability(capability) {
            return Err(AdapterGatewayError::UnknownAllowedCapability(
                capability.clone(),
            ));
        }
        validated.push(capability.clone());
    }
    validated.sort();
    Ok(validated)
}

fn validate_candidate_capabilities(
    capabilities: &[CapabilityRef],
    allowed: &HashSet<CapabilityRef>,
) -> Result<Vec<CapabilityRef>, AdapterGatewayError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(capabilities.len());
    for capability in capabilities {
        if !seen.insert(capability.clone()) || !allowed.contains(capability) {
            return Err(AdapterGatewayError::ForbiddenCandidateCapability(
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
) -> Result<Vec<VersionId>, AdapterGatewayError> {
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(versions.len());
    for version in versions {
        if !seen.insert(version.clone()) || !context.contains(version) {
            return Err(AdapterGatewayError::CandidateReferenceOutsideContext(
                version.clone(),
            ));
        }
        validated.push(version.clone());
    }
    validated.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(validated)
}

fn select_payload_fields(
    artifact: &Artifact,
    fields: &[String],
) -> Result<Value, AdapterGatewayError> {
    let payload = artifact.payload.as_object().ok_or_else(|| {
        AdapterGatewayError::ContextPayloadMustBeObject(artifact.version_id.clone())
    })?;
    let mut selected = Map::new();
    let mut seen = HashSet::new();
    for field in fields {
        if field.trim().is_empty() || !seen.insert(field) {
            return Err(AdapterGatewayError::InvalidContextField {
                version: artifact.version_id.clone(),
                field: field.clone(),
            });
        }
        let value = payload
            .get(field)
            .ok_or_else(|| AdapterGatewayError::InvalidContextField {
                version: artifact.version_id.clone(),
                field: field.clone(),
            })?;
        selected.insert(field.clone(), value.clone());
    }
    Ok(Value::Object(selected))
}

#[cfg(test)]
mod tests;

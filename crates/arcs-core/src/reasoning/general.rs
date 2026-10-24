//! Allgemeiner V2-Port; Solver und Modelle teilen keine erfundenen LLM-Metadaten.
use super::service::{
    ensure_candidate_schema, select_payload_fields, validate_allowed_capabilities,
    validate_candidate_capabilities, validate_candidate_references,
};
use super::{
    ReasoningContextItem, ReasoningError, ReasoningInvocationV2, ReasoningRequestV2,
    ReasoningResponseV2, ReasoningStatus,
};
use crate::adapters::{AdapterCallError, AdapterManifest, AdapterRegistry, ProducerClass};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactIdGenerator, Clock, Provenance, SchemaId, SchemaRegistry,
    Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId, validate_artifact,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_invocation_id,
};
use crate::store::{SqliteArtifactStore, relation_kinds};
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub trait GeneralReasoningAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;
    fn propose_v2(
        &self,
        invocation: &ReasoningInvocationV2,
    ) -> Result<ReasoningResponseV2, AdapterCallError>;
}

#[derive(Debug, serde::Serialize)]
pub struct ReasoningResult {
    pub status: ReasoningStatus,
    pub request_version_id: VersionId,
    pub result_version_id: VersionId,
    pub candidates: Vec<Artifact>,
}

pub struct GeneralReasoningService<'a> {
    pub registry: &'a AdapterRegistry,
    pub schemas: &'a SchemaRegistry,
    pub store: &'a SqliteArtifactStore,
    pub ids: &'a mut dyn ArtifactIdGenerator,
    pub clock: &'a dyn Clock,
}
fn invalid(message: &str) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}
fn invalid_response(message: &str) -> ReasoningError {
    ReasoningError::InvalidResponse(message.into())
}
fn minimum(a: TrustLevel, b: TrustLevel) -> TrustLevel {
    match (a, b) {
        (TrustLevel::Low, _) | (_, TrustLevel::Low) => TrustLevel::Low,
        (TrustLevel::Medium, _) | (_, TrustLevel::Medium) => TrustLevel::Medium,
        _ => TrustLevel::High,
    }
}

impl GeneralReasoningService<'_> {
    pub fn reason(
        &mut self,
        endpoint: &dyn GeneralReasoningAdapter,
        request: ReasoningRequestV2,
    ) -> Result<ReasoningResult, ReasoningError> {
        for schema in ["arcs.reasoning_request.v2", "arcs.reasoning_result.v2"] {
            if self.schemas.get(&SchemaId(schema.into())).is_none() {
                return Err(ReasoningError::MissingRegisteredSchema(SchemaId(
                    schema.into(),
                )));
            }
        }
        if request.request_id.trim().is_empty()
            || request.request_id.len() > 256
            || request.request_id.chars().any(char::is_control)
            || request.objective.trim().is_empty()
            || !request.budget.supports(request.method)
        {
            return Err(invalid(
                "invalid request identity, objective or resource budget",
            ));
        }
        let (registered, descriptor) = self.registry.authorized_capability(
            &request.capability.adapter_id,
            &request.capability.capability_id,
        )?;
        if endpoint.manifest() != registered.manifest() {
            return Err(invalid("reasoning endpoint manifest mismatch"));
        }
        if !descriptor.contract.is_reasoning() {
            return Err(invalid("capability is not Reason"));
        }
        let grant = registered.grant();
        let limits = grant
            .reasoning_limits_v2
            .as_ref()
            .ok_or(ReasoningError::ReasoningBudgetExceedsGrant)?;
        if !limits.methods.contains(&request.method)
            || !request.budget.fits_within(&limits.resources)
            || (request.method.uses_model() && grant.producer_class != ProducerClass::Model)
            || grant.producer_class == ProducerClass::Executor
        {
            return Err(ReasoningError::ReasoningBudgetExceedsGrant);
        }
        if !descriptor
            .contract
            .emitted_schemas()
            .contains(&request.target_schema_id)
        {
            return Err(invalid("undeclared reasoning output schema"));
        }
        ensure_candidate_schema(self.schemas, &request.target_schema_id)?;
        validate_allowed_capabilities(self.registry, &request.allowed_capabilities)?;
        if request.context.len() > request.budget.max_context_items {
            return Err(invalid("context item limit exceeded"));
        }
        let mut seen = HashSet::new();
        let mut context = Vec::new();
        let mut parents = Vec::new();
        let mut level = grant.assigned_trust;
        let mut model = grant.producer_class == ProducerClass::Model;
        let mut system = grant.producer_class == ProducerClass::System;
        for selection in &request.context {
            if !seen.insert(selection.version_id.clone()) {
                return Err(ReasoningError::DuplicateContextArtifact(
                    selection.version_id.clone(),
                ));
            }
            let input = self.store.get(&selection.version_id)?.ok_or_else(|| {
                ReasoningError::MissingContextArtifact(selection.version_id.clone())
            })?;
            validate_artifact(&input, self.schemas)
                .map_err(|_| invalid("invalid persisted context"))?;
            level = minimum(level, input.trust.level);
            model |= input.trust.source_class == SourceClass::Model
                || input.created_by.actor_type == ActorType::Model;
            system &= input.trust.source_class == SourceClass::System;
            parents.push(input.version_id.clone());
            context.push(ReasoningContextItem {
                payload: select_payload_fields(&input, &selection.payload_fields)?,
                version_id: input.version_id,
                schema_id: input.schema_id,
                artifact_type: input.artifact_type,
                trust_level: input.trust.level,
            });
        }
        let trust = Trust {
            level: if model { TrustLevel::Low } else { level },
            source_class: if model {
                SourceClass::Model
            } else if system {
                SourceClass::System
            } else {
                SourceClass::External
            },
        };
        let actor = match grant.producer_class {
            ProducerClass::Model => ActorType::Model,
            ProducerClass::System => ActorType::System,
            _ => ActorType::Adapter,
        };
        context.sort_by(|a, b| a.version_id.0.cmp(&b.version_id.0));
        let invocation_id = deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                "v2",
                &request.capability.adapter_id.0,
                &request.capability.capability_id.0,
                &request.request_id,
            ],
        );
        let invocation = ReasoningInvocationV2 {
            invocation_id: invocation_id.clone(),
            request_id: request.request_id,
            capability: request.capability,
            method: request.method,
            objective: request.objective,
            context,
            target_schema_id: request.target_schema_id,
            allowed_capabilities: request.allowed_capabilities,
            constraints: request.constraints,
            budget: request.budget,
        };
        if serde_json::to_vec(&invocation)?.len() > invocation.budget.max_context_bytes {
            return Err(invalid("serialized context budget exceeded"));
        }
        let fingerprint = serde_json::to_string(&(endpoint.manifest(), &invocation))?;
        let capability = format!(
            "{}/{}",
            invocation.capability.adapter_id.0, invocation.capability.capability_id.0
        );
        let existing =
            InvocationService::new(self.store, self.schemas, self.clock).lookup(&invocation_id)?;
        let prepared = if let Some(existing) = existing {
            InvocationService::new(self.store, self.schemas, self.clock).assert_identity(
                &existing,
                &InvocationSpec {
                    invocation_id: invocation_id.clone(),
                    kind: InvocationKind::Reasoning,
                    capability: capability.clone(),
                    input_version: existing.input_version.clone(),
                    input_fingerprint: fingerprint.clone(),
                },
            )?;
            existing
        } else {
            let audit = self.artifact(
                "arcs.reasoning_request.v2",
                serde_json::json!({"invocation":invocation}),
                &invocation_id,
                parents.clone(),
                None,
            );
            InvocationService::new(self.store, self.schemas, self.clock).prepare_with_event(
                InvocationSpec {
                    invocation_id: invocation_id.clone(),
                    kind: InvocationKind::Reasoning,
                    capability,
                    input_version: audit.version_id.clone(),
                    input_fingerprint: fingerprint,
                },
                &audit,
            )?
        };
        let result = if prepared.status == InvocationStatus::Succeeded {
            self.store
                .get(
                    prepared
                        .result_version
                        .as_ref()
                        .ok_or(crate::runtime::InvocationError::MissingResult)?,
                )?
                .ok_or(crate::runtime::InvocationError::MissingResult)?
        } else {
            let invocations = InvocationService::new(self.store, self.schemas, self.clock);
            let dispatched = invocations.dispatch(&invocations.recover(&prepared)?)?;
            let outcome = (|| {
                let started = Instant::now();
                let response = endpoint.propose_v2(&invocation)?;
                if started.elapsed() > Duration::from_millis(invocation.budget.max_runtime_ms) {
                    return Err(ReasoningError::AdapterCall(AdapterCallError::Timeout));
                }
                self.validate(&invocation, &response)?;
                let result = self.artifact(
                    "arcs.reasoning_result.v2",
                    serde_json::json!({"response":response}),
                    &invocation_id,
                    vec![prepared.input_version.clone()],
                    None,
                );
                InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                    &dispatched,
                    &result,
                    &[(prepared.input_version.clone(), relation_kinds::result_of())],
                )?;
                Ok(result)
            })();
            match outcome {
                Ok(result) => result,
                Err(error) => {
                    let retryable = matches!(
                        error,
                        ReasoningError::AdapterCall(
                            AdapterCallError::Timeout | AdapterCallError::Unavailable(_)
                        )
                    );
                    InvocationService::new(self.store, self.schemas, self.clock).fail(
                        &dispatched,
                        format!("{error:?}"),
                        retryable,
                    )?;
                    return Err(error);
                }
            }
        };
        validate_artifact(&result, self.schemas)
            .map_err(|_| invalid_response("invalid cached result artifact"))?;
        if result.schema_id.0 != "arcs.reasoning_result.v2"
            || result.created_by.id != "arcs.reasoning.v2"
        {
            return Err(invalid_response("wrong cached result identity"));
        }
        let response: ReasoningResponseV2 =
            serde_json::from_value(result.payload["response"].clone())?;
        self.validate(&invocation, &response)?;
        let mut candidates = Vec::new();
        for (index, proposal) in response.candidates.iter().enumerate() {
            let id = deterministic_invocation_id(
                InvocationKind::Reasoning,
                &["v2.candidate", &result.version_id.0, &index.to_string()],
            );
            let service = InvocationService::new(self.store, self.schemas, self.clock);
            let state = service.prepare(InvocationSpec {
                invocation_id: id.clone(),
                kind: InvocationKind::Reasoning,
                capability: format!(
                    "{}/{}",
                    invocation.capability.adapter_id.0, invocation.capability.capability_id.0
                ),
                input_version: result.version_id.clone(),
                input_fingerprint: serde_json::to_string(proposal)?,
            })?;
            if state.status == InvocationStatus::Succeeded {
                let cached = self
                    .store
                    .get(
                        state
                            .result_version
                            .as_ref()
                            .ok_or(crate::runtime::InvocationError::MissingResult)?,
                    )?
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                validate_artifact(&cached, self.schemas)
                    .map_err(|_| invalid_response("invalid cached candidate"))?;
                if cached.payload != proposal.payload
                    || cached.schema_id != proposal.schema_id
                    || cached.created_by.actor_type != actor
                    || cached.created_by.id != invocation.capability.adapter_id.0
                    || cached.trust.source_class != trust.source_class
                    || minimum(cached.trust.level, trust.level) != cached.trust.level
                {
                    return Err(invalid_response(
                        "cached candidate exceeds current identity or trust grant",
                    ));
                }
                candidates.push(cached);
                continue;
            }
            let dispatched = service.dispatch(&service.recover(&state)?)?;
            let mut candidate = self.artifact(
                &proposal.schema_id.0,
                proposal.payload.clone(),
                &id,
                proposal.referenced_versions.clone(),
                Some((
                    Actor {
                        actor_type: actor,
                        id: invocation.capability.adapter_id.0.clone(),
                    },
                    trust.clone(),
                )),
            );
            candidate.tags = proposal
                .required_capabilities
                .iter()
                .map(|cap| format!("requires:{}:{}", cap.adapter_id.0, cap.capability_id.0))
                .collect();
            let relations = proposal
                .referenced_versions
                .iter()
                .cloned()
                .map(|id| (id, relation_kinds::supported_by()))
                .chain([
                    (
                        prepared.input_version.clone(),
                        relation_kinds::generated_by(),
                    ),
                    (result.version_id.clone(), relation_kinds::derived_from()),
                ])
                .collect::<Vec<_>>();
            InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                &dispatched,
                &candidate,
                &relations,
            )?;
            candidates.push(candidate);
        }
        Ok(ReasoningResult {
            status: response.status,
            request_version_id: prepared.input_version,
            result_version_id: result.version_id,
            candidates,
        })
    }

    fn validate(
        &self,
        request: &ReasoningInvocationV2,
        response: &ReasoningResponseV2,
    ) -> Result<(), ReasoningError> {
        if response.invocation_id != request.invocation_id
            || response.request_id != request.request_id
            || response.trace.method != request.method
            || response.candidates.len() > request.budget.max_candidates
            || (response.status == ReasoningStatus::Solved) == response.candidates.is_empty()
        {
            return Err(invalid_response(
                "reasoning correlation, status or candidate count mismatch",
            ));
        }
        if serde_json::to_vec(response)?.len() > request.budget.max_output_bytes {
            return Err(invalid_response("reasoning response exceeds output limit"));
        }
        for name in [&response.trace.implementation, &response.trace.version] {
            if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
                return Err(invalid_response("invalid implementation trace"));
            }
        }
        if response
            .trace
            .iterations
            .is_some_and(|value| request.budget.max_iterations.is_none_or(|max| value > max))
            || response.trace.output_tokens.is_some_and(|value| {
                request
                    .budget
                    .max_output_tokens
                    .is_none_or(|max| value > max)
            })
            || (matches!(
                request.method,
                super::ReasoningMethod::Solver | super::ReasoningMethod::Rules
            ) && response.trace.iterations.is_none())
            || (request.method == super::ReasoningMethod::Llm
                && response.trace.output_tokens.is_none())
        {
            return Err(invalid_response(
                "reported resource usage exceeds budget or is missing",
            ));
        }
        let versions = request
            .context
            .iter()
            .map(|item| item.version_id.clone())
            .collect::<HashSet<_>>();
        let allowed = request
            .allowed_capabilities
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let (registered, _) = self.registry.authorized_capability(
            &request.capability.adapter_id,
            &request.capability.capability_id,
        )?;
        for proposal in &response.candidates {
            if serde_json::to_vec(&proposal.payload)?.len() > registered.grant().max_payload_bytes {
                return Err(invalid_response("candidate exceeds grant payload limit"));
            }
            if proposal.schema_id != request.target_schema_id {
                return Err(ReasoningError::UnexpectedCandidateSchema(
                    proposal.schema_id.clone(),
                ));
            }
            ensure_candidate_schema(self.schemas, &proposal.schema_id)?;
            self.schemas
                .validate(&proposal.schema_id, &proposal.payload)
                .map_err(ReasoningError::InvalidPayload)?;
            validate_candidate_capabilities(&proposal.required_capabilities, &allowed)?;
            validate_candidate_references(&proposal.referenced_versions, &versions)?;
        }
        Ok(())
    }

    fn artifact(
        &mut self,
        schema: &str,
        payload: serde_json::Value,
        key: &str,
        parents: Vec<VersionId>,
        producer: Option<(Actor, Trust)>,
    ) -> Artifact {
        let definition = self
            .schemas
            .get(&SchemaId(schema.into()))
            .expect("schema checked before artifact creation");
        let generated = self.ids.next(&definition.artifact_type);
        let internal = producer.is_none();
        let (actor, trust) = producer.unwrap_or((
            Actor {
                actor_type: ActorType::System,
                id: "arcs.reasoning.v2".into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
        ));
        Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type.clone(),
            schema_id: definition.id.clone(),
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: actor,
            source: Source {
                kind: if internal {
                    SourceKind::Internal
                } else {
                    SourceKind::External
                },
                reference: format!("reasoning-v2:{key}"),
            },
            trust,
            stream_key: format!("reasoning-v2:{key}"),
            subject: None,
            tags: vec![],
            payload,
            provenance: Some(Provenance {
                parents: parents.into_iter().map(|id| id.0).collect(),
                rules_applied: vec!["reasoning.v2.validated".into()],
                models_used: vec![],
                transform: Some("reasoning.v2".into()),
            }),
        }
    }
}

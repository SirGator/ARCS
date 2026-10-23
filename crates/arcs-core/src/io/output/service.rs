use serde::{Deserialize, Serialize};

use super::{OutputAdapter, OutputError, OutputInvocation};
use crate::adapters::registration::RegisteredAdapter;
use crate::adapters::{
    AdapterCallError, AdapterManifest, AdapterRegistry, CapabilityContract, CapabilityRef,
    ProducerClass,
};
use crate::approval::ApprovalDecision;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId, validate_artifact,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_invocation_id,
};
use crate::store::{Idempotency, SqliteArtifactStore, relation_kinds};
use crate::verification::VerificationVerdict;

const REQUEST_SCHEMA: &str = "arcs.output_request.v1";

/// Der Core bindet die Freigabe an diese konkrete Ausgabeabsicht.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputRequest {
    pub artifact_version_id: VersionId,
    pub artifact_schema_id: SchemaId,
    pub capability: CapabilityRef,
    pub result_schema_id: SchemaId,
    pub manifest: AdapterManifest,
}

pub struct OutputService<'a> {
    registry: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> OutputService<'a> {
    pub fn new(
        registry: &'a AdapterRegistry,
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            registry,
            schemas,
            store,
            ids,
            clock,
        }
    }

    fn load(&self, version: &VersionId) -> Result<Artifact, OutputError> {
        let artifact = self
            .store
            .get(version)?
            .ok_or_else(|| OutputError::MissingArtifact(version.clone()))?;
        validate_artifact(&artifact, self.schemas).map_err(OutputError::InvalidArtifact)?;
        Ok(artifact)
    }

    /// Erstellt eine idempotente Ausgabeabsicht, noch ohne Lieferwirkung.
    pub fn prepare(
        &mut self,
        capability: &CapabilityRef,
        input_version: &VersionId,
        result_schema: &SchemaId,
    ) -> Result<Artifact, OutputError> {
        let input = self.load(input_version)?;
        let registered = authorize(
            self.registry,
            self.schemas,
            capability,
            &input,
            result_schema,
        )?;
        let request = OutputRequest {
            artifact_version_id: input.version_id.clone(),
            artifact_schema_id: input.schema_id.clone(),
            capability: capability.clone(),
            result_schema_id: result_schema.clone(),
            manifest: registered.manifest().clone(),
        };
        let payload = serde_json::to_value(&request)?;
        let input_identity = serde_json::to_string(&payload)?;
        let key = deterministic_invocation_id(
            InvocationKind::Output,
            &[
                "request",
                &capability.adapter_id.0,
                &capability.capability_id.0,
                &input_version.0,
                &result_schema.0,
                &request.manifest.adapter_version,
            ],
        );
        let stream = format!("output-request:{key}");
        let schema = self
            .schemas
            .get(&SchemaId(REQUEST_SCHEMA.into()))
            .ok_or(OutputError::Gate("missing output request schema"))?
            .clone();
        let artifact = ArtifactFactory::new(self.clock, self.ids).create(ArtifactFactoryInput {
            schema,
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.output".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: stream.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: stream.clone(),
            subject: SubjectId(stream.clone()),
            tags: vec![format!(
                "output:{}:{}",
                capability.adapter_id.0, capability.capability_id.0
            )],
            payload,
            provenance: Some(Provenance {
                parents: vec![input.version_id.0.clone()],
                rules_applied: vec!["output.request_bound".into()],
                models_used: vec![],
                transform: Some("output.prepare".into()),
            }),
        })?;
        Ok(self
            .store
            .append_current_related_once(
                &artifact,
                self.schemas,
                Idempotency {
                    key: &stream,
                    input: &input_identity,
                },
                &[(input.version_id, relation_kinds::derived_from())],
            )?
            .artifact)
    }

    /// Liefert ausschließlich einen geprüften und freigegebenen Ausgabeauftrag.
    /// Die aktuelle Autorisierung wird auch vor einem Cache-Replay erneut geprüft.
    pub fn deliver(
        &mut self,
        endpoint: &dyn OutputAdapter,
        request_version: &VersionId,
        approval_version: &VersionId,
    ) -> Result<Artifact, OutputError> {
        let request_artifact = self.load(request_version)?;
        if request_artifact.schema_id.0 != REQUEST_SCHEMA
            || request_artifact.created_by.actor_type != ActorType::System
            || request_artifact.created_by.id != "arcs.output"
            || request_artifact.source.kind != SourceKind::Internal
            || request_artifact.trust.level != TrustLevel::High
            || request_artifact.trust.source_class != SourceClass::System
        {
            return Err(OutputError::Gate("not a core output request"));
        }
        let request: OutputRequest = serde_json::from_value(request_artifact.payload.clone())?;
        let input = self.load(&request.artifact_version_id)?;
        let registered = authorize(
            self.registry,
            self.schemas,
            &request.capability,
            &input,
            &request.result_schema_id,
        )?;
        if input.schema_id != request.artifact_schema_id
            || &request.manifest != registered.manifest()
            || endpoint.manifest() != registered.manifest()
        {
            return Err(OutputError::ContractMismatch);
        }
        if !has_relation(
            self.store,
            request_version,
            &input.version_id,
            relation_kinds::derived_from(),
        )? {
            return Err(OutputError::Gate("missing output input relation"));
        }
        self.check_approval(request_version, approval_version)?;
        let grant = registered.grant();
        let id = deterministic_invocation_id(
            InvocationKind::Output,
            &[
                "deliver",
                &request_version.0,
                &approval_version.0,
                &request.capability.adapter_id.0,
                &request.capability.capability_id.0,
                &request.result_schema_id.0,
            ],
        );
        let invocation = OutputInvocation {
            invocation_id: id.clone(),
            capability: request.capability.clone(),
            request_version_id: request_version.clone(),
            approval_version_id: approval_version.clone(),
            artifact_version_id: input.version_id.clone(),
            artifact_schema_id: input.schema_id.clone(),
            subject: input.subject.clone(),
            payload: input.payload.clone(),
            result_schema_id: request.result_schema_id.clone(),
            max_result_bytes: grant.max_payload_bytes,
        };
        let fingerprint = serde_json::to_string(&invocation)?;
        let dispatched = {
            let lifecycle = InvocationService::new(self.store, self.schemas, self.clock);
            let state = lifecycle.prepare(InvocationSpec {
                invocation_id: id.clone(),
                kind: InvocationKind::Output,
                capability: format!(
                    "{}/{}",
                    request.capability.adapter_id.0, request.capability.capability_id.0
                ),
                input_version: request_version.clone(),
                input_fingerprint: fingerprint,
            })?;
            if state.status == InvocationStatus::Succeeded {
                let result = self.load(
                    &state
                        .result_version
                        .ok_or(crate::runtime::InvocationError::MissingResult)?,
                )?;
                if result.schema_id != request.result_schema_id
                    || result.created_by.actor_type != ActorType::Executor
                    || result.created_by.id != request.capability.adapter_id.0
                    || result.trust.source_class != SourceClass::External
                    || matches!(
                        (grant.assigned_trust, result.trust.level),
                        (TrustLevel::Low, TrustLevel::Medium | TrustLevel::High)
                            | (TrustLevel::Medium, TrustLevel::High)
                    )
                    || serde_json::to_vec(&result.payload)?.len() > grant.max_payload_bytes
                    || result.source.reference.len() > grant.max_external_reference_bytes
                {
                    return Err(OutputError::CachedResultNotPermitted);
                }
                return Ok(result);
            }
            lifecycle.dispatch(&lifecycle.recover(&state)?)?
        };
        let outcome = (|| {
            let response = endpoint.deliver(&invocation)?;
            if response.invocation_id != id
                || response.external_reference.trim().is_empty()
                || response.external_reference.len() > grant.max_external_reference_bytes
                || response.external_reference.chars().any(char::is_control)
            {
                return Err(OutputError::InvalidResponse);
            }
            if serde_json::to_vec(&response.result_payload)?.len() > grant.max_payload_bytes {
                return Err(OutputError::ResultTooLarge);
            }
            self.schemas
                .validate(&request.result_schema_id, &response.result_payload)
                .map_err(|_| OutputError::InvalidResponse)?;
            let artifact =
                ArtifactFactory::new(self.clock, self.ids).create(ArtifactFactoryInput {
                    schema: self
                        .schemas
                        .get(&request.result_schema_id)
                        .ok_or(OutputError::ContractMismatch)?
                        .clone(),
                    created_by: Actor {
                        actor_type: ActorType::Executor,
                        id: request.capability.adapter_id.0.clone(),
                    },
                    source: Source {
                        kind: SourceKind::External,
                        reference: response.external_reference,
                    },
                    trust: Trust {
                        level: grant.assigned_trust,
                        source_class: SourceClass::External,
                    },
                    stream_key: format!("output:{id}"),
                    subject: SubjectId(format!("output:{}", request_version.0)),
                    tags: vec![
                        format!("output:{}", request_version.0),
                        format!("approval:{}", approval_version.0),
                    ],
                    payload: response.result_payload,
                    provenance: Some(Provenance {
                        parents: vec![
                            input.version_id.0.clone(),
                            request_version.0.clone(),
                            approval_version.0.clone(),
                        ],
                        rules_applied: vec![
                            "output.request_validated".into(),
                            "output.approval_checked".into(),
                            "output.capability_authorized".into(),
                            "output.receipt_validated".into(),
                        ],
                        models_used: vec![],
                        transform: Some("output.deliver".into()),
                    }),
                })?;
            InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                &dispatched,
                &artifact,
                &[
                    (request_version.clone(), relation_kinds::result_of()),
                    (approval_version.clone(), relation_kinds::based_on()),
                    (input.version_id.clone(), relation_kinds::derived_from()),
                ],
            )?;
            Ok(artifact)
        })();
        if let Err(error) = &outcome {
            let retryable = matches!(
                error,
                OutputError::AdapterCall(
                    AdapterCallError::Timeout | AdapterCallError::Unavailable(_)
                )
            );
            InvocationService::new(self.store, self.schemas, self.clock).fail(
                &dispatched,
                format!("{error:?}"),
                retryable,
            )?;
        }
        outcome
    }

    fn check_approval(&self, request: &VersionId, approval: &VersionId) -> Result<(), OutputError> {
        #[derive(Deserialize)]
        struct Approval {
            target_version: String,
            verification_report_version: String,
            decision: ApprovalDecision,
        }
        #[derive(Deserialize)]
        struct Verification {
            target_version: String,
            verdict: VerificationVerdict,
        }
        let approval_artifact = self.load(approval)?;
        if approval_artifact.schema_id.0 != "arcs.approval.v1"
            || !matches!(
                approval_artifact.created_by.actor_type,
                ActorType::Human | ActorType::System
            )
            || approval_artifact.source.kind != SourceKind::Internal
            || approval_artifact.trust.level != TrustLevel::High
            || !matches!(
                (
                    approval_artifact.created_by.actor_type,
                    approval_artifact.trust.source_class
                ),
                (ActorType::Human, SourceClass::Human) | (ActorType::System, SourceClass::System)
            )
        {
            return Err(OutputError::Gate("untrusted output approval"));
        }
        let gate: Approval = serde_json::from_value(approval_artifact.payload)?;
        if gate.decision != ApprovalDecision::Approved || gate.target_version != request.0 {
            return Err(OutputError::Gate("output request was not approved"));
        }
        let verification = VersionId(gate.verification_report_version);
        let report = self.load(&verification)?;
        if report.schema_id.0 != "arcs.verification_report.v1"
            || report.created_by.actor_type != ActorType::System
            || report.source.kind != SourceKind::Internal
            || report.trust.level != TrustLevel::High
            || report.trust.source_class != SourceClass::System
        {
            return Err(OutputError::Gate("untrusted output verification"));
        }
        let checked: Verification = serde_json::from_value(report.payload)?;
        if checked.target_version != request.0
            || checked.verdict != VerificationVerdict::Pass
            || !has_relation(self.store, approval, request, relation_kinds::approves())?
            || !has_relation(
                self.store,
                approval,
                &verification,
                relation_kinds::based_on(),
            )?
            || !has_relation(
                self.store,
                &verification,
                request,
                relation_kinds::verifies(),
            )?
        {
            return Err(OutputError::Gate(
                "output verification or authority relations do not match",
            ));
        }
        Ok(())
    }
}

fn authorize<'a>(
    registry: &'a AdapterRegistry,
    schemas: &SchemaRegistry,
    capability: &CapabilityRef,
    input: &Artifact,
    result: &SchemaId,
) -> Result<&'a RegisteredAdapter, OutputError> {
    let (registered, descriptor) =
        registry.authorized_capability(&capability.adapter_id, &capability.capability_id)?;
    if registered.grant().producer_class != ProducerClass::Executor {
        return Err(OutputError::ExecutorRequired);
    }
    if !matches!(&descriptor.contract,CapabilityContract::Output {accepts,emits,idempotent:true} if accepts.contains(&input.schema_id)&&emits.contains(result))
    {
        return Err(OutputError::ContractMismatch);
    }
    let definition = schemas.get(result).ok_or(OutputError::ContractMismatch)?;
    if super::super::control_types::is_control_type(&definition.artifact_type) {
        return Err(OutputError::ControlResultForbidden);
    }
    if serde_json::to_vec(&input.payload)?.len() > registered.grant().max_payload_bytes {
        return Err(OutputError::InputTooLarge);
    }
    Ok(registered)
}

fn has_relation(
    store: &SqliteArtifactStore,
    from: &VersionId,
    to: &VersionId,
    kind: crate::store::RelationKind,
) -> Result<bool, OutputError> {
    Ok(store
        .outgoing_relations(from)?
        .iter()
        .any(|relation| relation.to == *to && relation.kind == kind))
}

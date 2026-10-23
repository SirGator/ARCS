use crate::adapters::{
    AdapterCallError, AdapterRegistry, CapabilityContract, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId, validate_artifact,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_invocation_id,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::{TransformAdapter, TransformError, TransformInvocation};

pub struct TransformService<'a> {
    registry: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> TransformService<'a> {
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

    /// Validiert den gesamten gespeicherten Input vor jedem Aufruf, auch beim
    /// Replay. Resultat, Relation und erfolgreicher Invocation-State committen atomar.
    pub fn execute(
        &mut self,
        endpoint: &dyn TransformAdapter,
        capability: &CapabilityRef,
        input_version: &VersionId,
        output_schema: &SchemaId,
    ) -> Result<Artifact, TransformError> {
        let (registered, descriptor) = self
            .registry
            .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
        if endpoint.manifest() != registered.manifest() {
            return Err(TransformError::EndpointMismatch);
        }
        let CapabilityContract::Transform { accepts, emits } = &descriptor.contract else {
            return Err(TransformError::CapabilityIsNotTransform(capability.clone()));
        };
        let grant = registered.grant();
        if grant.producer_class == ProducerClass::Model {
            return Err(TransformError::ModelMustUseReasoning);
        }
        let input = self
            .store
            .get(input_version)?
            .ok_or_else(|| TransformError::MissingInput(input_version.clone()))?;
        validate_artifact(&input, self.schemas).map_err(TransformError::InvalidInput)?;
        if !accepts.contains(&input.schema_id) {
            return Err(TransformError::InputSchemaNotAccepted(
                input.schema_id.clone(),
            ));
        }
        if !emits.contains(output_schema) {
            return Err(TransformError::UndeclaredOutputSchema(
                output_schema.clone(),
            ));
        }
        let definition = self
            .schemas
            .get(output_schema)
            .ok_or_else(|| TransformError::MissingSchema(output_schema.clone()))?
            .clone();
        if super::super::control_types::is_control_type(&definition.artifact_type) {
            return Err(TransformError::ControlOutputForbidden(
                output_schema.clone(),
            ));
        }
        if serde_json::to_vec(&input.payload)?.len() > grant.max_payload_bytes {
            return Err(TransformError::InputTooLarge);
        }
        let result_trust = derived_trust(&input, grant.producer_class, grant.assigned_trust);
        let actor_type = match grant.producer_class {
            ProducerClass::System => ActorType::System,
            ProducerClass::Executor => ActorType::Executor,
            ProducerClass::Adapter => ActorType::Adapter,
            ProducerClass::Model => unreachable!("models rejected before dispatch"),
        };
        let invocation_id = deterministic_invocation_id(
            InvocationKind::Transform,
            &[
                &capability.adapter_id.0,
                &capability.capability_id.0,
                &input_version.0,
                &output_schema.0,
                &endpoint.manifest().adapter_version,
            ],
        );
        // Vollständige Identität zusätzlich zum kurzen Routing-Schlüssel. Ein
        // Schlüsselkonflikt darf niemals ein fremdes Cache-Ergebnis liefern.
        let fingerprint = serde_json::to_string(&(
            endpoint.manifest(),
            capability,
            input_version,
            &input.schema_id,
            &input.payload,
            output_schema,
        ))?;
        let dispatched = {
            let service = InvocationService::new(self.store, self.schemas, self.clock);
            let state = service.prepare(InvocationSpec {
                invocation_id: invocation_id.clone(),
                kind: InvocationKind::Transform,
                capability: format!("{}/{}", capability.adapter_id.0, capability.capability_id.0),
                input_version: input.version_id.clone(),
                input_fingerprint: fingerprint,
            })?;
            if state.status == InvocationStatus::Succeeded {
                let result = state
                    .result_version
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                let result = self
                    .store
                    .get(&result)?
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                if validate_artifact(&result, self.schemas).is_err()
                    || result.schema_id != *output_schema
                    || result.created_by.actor_type != actor_type
                    || result.created_by.id != capability.adapter_id.0
                    || matches!(
                        (result_trust.level, result.trust.level),
                        (TrustLevel::Low, TrustLevel::Medium | TrustLevel::High)
                            | (TrustLevel::Medium, TrustLevel::High)
                    )
                    || (result_trust.source_class == SourceClass::Model
                        && result.trust.source_class != SourceClass::Model)
                    || serde_json::to_vec(&result.payload)?.len() > grant.max_payload_bytes
                    || result.source.reference.len() > grant.max_external_reference_bytes
                {
                    return Err(TransformError::CachedResultNotPermitted);
                }
                return Ok(result);
            }
            service.dispatch(&service.recover(&state)?)?
        };
        let invocation = TransformInvocation {
            invocation_id: invocation_id.clone(),
            capability: capability.clone(),
            input_version_id: input.version_id.clone(),
            input_schema_id: input.schema_id.clone(),
            input_payload: input.payload.clone(),
            output_schema_id: output_schema.clone(),
            max_output_bytes: grant.max_payload_bytes,
        };
        let outcome = (|| {
            let response = endpoint.transform(&invocation)?;
            if response.invocation_id != invocation_id {
                return Err(TransformError::InvocationResponseMismatch);
            }
            if response.external_reference.trim().is_empty()
                || response.external_reference.len() > grant.max_external_reference_bytes
                || response.external_reference.chars().any(char::is_control)
            {
                return Err(TransformError::InvalidExternalReference);
            }
            if serde_json::to_vec(&response.payload)?.len() > grant.max_payload_bytes {
                return Err(TransformError::OutputTooLarge);
            }
            self.schemas
                .validate(output_schema, &response.payload)
                .map_err(TransformError::InvalidOutput)?;
            let artifact =
                ArtifactFactory::new(self.clock, self.ids).create(ArtifactFactoryInput {
                    schema: definition,
                    created_by: Actor {
                        actor_type,
                        id: capability.adapter_id.0.clone(),
                    },
                    source: Source {
                        kind: if grant.producer_class == ProducerClass::System {
                            SourceKind::Internal
                        } else {
                            SourceKind::External
                        },
                        reference: response.external_reference,
                    },
                    trust: result_trust,
                    stream_key: format!("transform:{invocation_id}"),
                    subject: input
                        .subject
                        .clone()
                        .unwrap_or_else(|| SubjectId(format!("transform:{invocation_id}"))),
                    tags: vec![
                        format!("adapter:{}", capability.adapter_id.0),
                        format!("capability:{}", capability.capability_id.0),
                        format!("input:{}", input.version_id.0),
                    ],
                    payload: response.payload,
                    provenance: Some(Provenance {
                        parents: vec![input.version_id.0.clone()],
                        rules_applied: vec![
                            "transform.capability_authorized".into(),
                            "transform.input_validated".into(),
                            "transform.output_validated".into(),
                            "transform.trust_not_elevated".into(),
                        ],
                        models_used: vec![],
                        transform: Some(format!(
                            "{}/{}@{}",
                            capability.adapter_id.0,
                            capability.capability_id.0,
                            endpoint.manifest().adapter_version
                        )),
                    }),
                })?;
            InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                &dispatched,
                &artifact,
                &[(input.version_id.clone(), relation_kinds::derived_from())],
            )?;
            Ok(artifact)
        })();
        if let Err(error) = &outcome {
            let retryable = matches!(
                error,
                TransformError::AdapterCall(
                    AdapterCallError::Unavailable(_) | AdapterCallError::Timeout
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
}

fn derived_trust(input: &Artifact, producer: ProducerClass, assigned: TrustLevel) -> Trust {
    let model_origin = input.trust.source_class == SourceClass::Model
        || input.created_by.actor_type == ActorType::Model;
    let level = match (input.trust.level, assigned) {
        _ if model_origin => TrustLevel::Low,
        (TrustLevel::Low, _) | (_, TrustLevel::Low) => TrustLevel::Low,
        (TrustLevel::Medium, _) | (_, TrustLevel::Medium) => TrustLevel::Medium,
        _ => TrustLevel::High,
    };
    let source_class = if model_origin {
        SourceClass::Model
    } else if producer == ProducerClass::System {
        input.trust.source_class
    } else {
        SourceClass::External
    };
    Trust {
        level,
        source_class,
    }
}

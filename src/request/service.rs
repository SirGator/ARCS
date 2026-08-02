use std::collections::HashSet;

use crate::adapters::{AdapterRegistry, CapabilityContract, CapabilityRef, ProducerClass};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::{RequestAdapter, RequestError, RequestInvocation};

/// Eigenständiger Slice für korrelierte Datenanforderungen.
///
/// Der Service kennt weder Observation-, Reasoning- noch Output-Endpoints.
/// Ein konkreter Microservice wird nur für den jeweiligen Aufruf als Port
/// übergeben und erhält keinen Zugriff auf ARCS-Ressourcen.
pub struct RequestService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
    completed: HashSet<(CapabilityRef, VersionId, SchemaId)>,
}

impl<'a> RequestService<'a> {
    pub fn new(
        policy: &'a AdapterRegistry,
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            policy,
            schemas,
            store,
            ids,
            clock,
            completed: HashSet::new(),
        }
    }

    pub fn execute(
        &mut self,
        endpoint: &dyn RequestAdapter,
        capability: &CapabilityRef,
        request_version: &VersionId,
        response_schema: &SchemaId,
    ) -> Result<Artifact, RequestError> {
        let completion_key = (
            capability.clone(),
            request_version.clone(),
            response_schema.clone(),
        );
        if self.completed.contains(&completion_key) {
            return Err(RequestError::InvocationAlreadyCompleted {
                capability: capability.clone(),
                input: request_version.clone(),
                response_schema: response_schema.clone(),
            });
        }

        if endpoint.manifest().adapter_id != capability.adapter_id {
            return Err(RequestError::Authorization(
                crate::adapters::AdapterRegistryError::UnknownAdapter(
                    endpoint.manifest().adapter_id.clone(),
                ),
            ));
        }

        let request = self
            .store
            .get(request_version)?
            .ok_or_else(|| RequestError::MissingInputArtifact(request_version.clone()))?;
        let subject = request
            .subject
            .clone()
            .ok_or_else(|| RequestError::MissingRequestSubject(request_version.clone()))?;

        let (registered, descriptor) = self
            .policy
            .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
        let CapabilityContract::Request { accepts, emits } = &descriptor.contract else {
            return Err(RequestError::CapabilityIsNotRequest(capability.clone()));
        };
        if registered.grant().producer_class == ProducerClass::Model {
            return Err(RequestError::ModelMustNotServeRequests(
                capability.adapter_id.clone(),
            ));
        }
        if !accepts.contains(&request.schema_id) {
            return Err(RequestError::InputSchemaNotAccepted {
                capability: capability.clone(),
                schema: request.schema_id.clone(),
            });
        }
        if !emits.contains(response_schema) {
            return Err(RequestError::UndeclaredResponseSchema {
                capability: capability.capability_id.clone(),
                schema: response_schema.clone(),
            });
        }

        let grant = registered.grant();
        let source_kind = grant
            .observation_source_kind
            .ok_or(RequestError::MissingSourceKind)?;
        let definition = self
            .schemas
            .get(response_schema)
            .ok_or_else(|| RequestError::MissingRegisteredSchema(response_schema.clone()))?
            .clone();
        let invocation_id = invocation_id(capability, request_version, response_schema);
        let invocation = RequestInvocation {
            invocation_id: invocation_id.clone(),
            capability: capability.clone(),
            request_version_id: request.version_id.clone(),
            request_schema_id: request.schema_id.clone(),
            subject: subject.clone(),
            request_payload: request.payload.clone(),
            response_schema_id: response_schema.clone(),
            max_response_bytes: grant.max_payload_bytes,
        };

        let response = endpoint.fetch(&invocation)?;
        if response.invocation_id != invocation_id {
            return Err(RequestError::InvocationResponseMismatch);
        }
        if response.external_reference.trim().is_empty() {
            return Err(RequestError::InvalidExternalReference);
        }
        let reference_size = response.external_reference.len();
        if reference_size > grant.max_external_reference_bytes {
            return Err(RequestError::ExternalReferenceTooLarge {
                actual: reference_size,
                maximum: grant.max_external_reference_bytes,
            });
        }
        let payload_size = serde_json::to_vec(&response.payload)?.len();
        if payload_size > grant.max_payload_bytes {
            return Err(RequestError::PayloadTooLarge {
                actual: payload_size,
                maximum: grant.max_payload_bytes,
            });
        }
        self.schemas
            .validate(response_schema, &response.payload)
            .map_err(RequestError::InvalidPayload)?;

        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let artifact = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: Actor {
                actor_type: actor_type_for(grant.producer_class),
                id: capability.adapter_id.0.clone(),
            },
            source: Source {
                kind: source_kind,
                reference: response.external_reference,
            },
            trust: trust_for(grant.producer_class, grant.assigned_trust),
            stream_key: request.stream_key.clone(),
            subject,
            tags: vec![
                format!("adapter:{}", capability.adapter_id.0),
                format!("capability:{}", capability.capability_id.0),
                format!("request:{}", request.version_id.0),
            ],
            payload: response.payload,
            provenance: Some(Provenance {
                parents: vec![request.version_id.0.clone()],
                rules_applied: vec![
                    "request.capability_authorized".into(),
                    "request.response_correlated".into(),
                    "request.payload_schema_validated".into(),
                ],
                models_used: vec![],
                transform: Some(format!("request:{}", capability.adapter_id.0)),
            }),
        })?;

        self.store.append_current_related(
            &artifact,
            self.schemas,
            &[
                (request.version_id.clone(), relation_kinds::fulfills()),
                (request.version_id, relation_kinds::caused_by()),
            ],
        )?;
        self.completed.insert(completion_key);
        Ok(artifact)
    }
}

fn invocation_id(
    capability: &CapabilityRef,
    request: &VersionId,
    response_schema: &SchemaId,
) -> String {
    format!(
        "request:{}:{}:{}:{}:{}:{}:{}:{}",
        capability.adapter_id.0.len(),
        capability.adapter_id.0,
        capability.capability_id.0.len(),
        capability.capability_id.0,
        request.0.len(),
        request.0,
        response_schema.0.len(),
        response_schema.0
    )
}

fn actor_type_for(producer: ProducerClass) -> ActorType {
    match producer {
        ProducerClass::Adapter => ActorType::Adapter,
        ProducerClass::Model => ActorType::Model,
        ProducerClass::System => ActorType::System,
        ProducerClass::Executor => ActorType::Executor,
    }
}

fn trust_for(producer: ProducerClass, assigned: TrustLevel) -> Trust {
    match producer {
        ProducerClass::Model => Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        ProducerClass::System => Trust {
            level: assigned,
            source_class: SourceClass::System,
        },
        ProducerClass::Adapter | ProducerClass::Executor => Trust {
            level: assigned,
            source_class: SourceClass::External,
        },
    }
}

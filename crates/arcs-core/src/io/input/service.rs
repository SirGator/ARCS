//! Use Case für einmalige, aktive Ereignisse an externen Eingangsgrenzen.

use crate::adapters::{
    AdapterId, AdapterRegistry, AdapterRegistryError, CapabilityContract, CapabilityRef,
    ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaRegistry, Source, SourceClass, SubjectId, Trust, TrustLevel,
};
use crate::store::SqliteArtifactStore;

use super::{InputError, InputMessage};

/// Kontrollierte Ingress-Grenze für historische Input-Ereignisse.
pub struct InputService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> InputService<'a> {
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
        }
    }

    /// Validiert einen externen Input und speichert ihn als historisches Ereignis.
    pub fn ingest(
        &mut self,
        adapter_id: &AdapterId,
        message: InputMessage,
    ) -> Result<Artifact, InputError> {
        if message.external_reference.trim().is_empty() {
            return Err(InputError::InvalidExternalReference);
        }

        let (registered, descriptor) = self
            .policy
            .authorized_capability(adapter_id, &message.capability_id)?;
        let capability = CapabilityRef {
            adapter_id: adapter_id.clone(),
            capability_id: message.capability_id.clone(),
        };
        let CapabilityContract::Input { emits } = &descriptor.contract else {
            return Err(InputError::CapabilityIsNotInput(capability));
        };
        if emits.len() != 1 {
            return Err(InputError::InvalidInputSchemaCount {
                capability,
                actual: emits.len(),
            });
        }
        let schema_id = emits[0].clone();

        let grant = registered.grant();
        let producer_class = grant.producer_class;
        let assigned_trust = grant.assigned_trust;
        let source_kind = grant
            .ingress_source_kind
            .ok_or(AdapterRegistryError::MissingIngressSource)?;
        let maximum_payload = grant.max_payload_bytes;
        let maximum_reference = grant.max_external_reference_bytes;

        let reference_size = message.external_reference.len();
        if reference_size > maximum_reference {
            return Err(InputError::ExternalReferenceTooLarge {
                actual: reference_size,
                maximum: maximum_reference,
            });
        }
        let payload_size = serde_json::to_vec(&message.payload)?.len();
        if payload_size > maximum_payload {
            return Err(InputError::PayloadTooLarge {
                actual: payload_size,
                maximum: maximum_payload,
            });
        }

        self.schemas
            .validate(&schema_id, &message.payload)
            .map_err(InputError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| InputError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();

        let external_subject = message
            .external_subject
            .filter(|subject| !subject.trim().is_empty())
            .ok_or(InputError::MissingExternalSubject)?;
        let subject = input_subject(adapter_id, &message.capability_id.0, &external_subject);
        let stream_key = input_stream_key(
            adapter_id,
            &message.capability_id.0,
            &message.external_reference,
        );

        if let Some(existing) = self.store.find_by_stream_key(&stream_key)? {
            let same_input = existing.schema_id == definition.id
                && existing.subject.as_ref() == Some(&subject)
                && existing.source.reference == message.external_reference
                && existing.payload == message.payload;
            if same_input {
                return Ok(existing);
            }
            return Err(InputError::IdentityConflict(stream_key));
        }

        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let artifact = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: Actor {
                actor_type: actor_type_for(producer_class),
                id: adapter_id.0.clone(),
            },
            source: Source {
                kind: source_kind,
                reference: message.external_reference,
            },
            trust: trust_for(producer_class, assigned_trust),
            stream_key,
            subject,
            tags: vec![
                format!("adapter:{}", adapter_id.0),
                format!("capability:{}", message.capability_id.0),
            ],
            payload: message.payload,
            provenance: Some(Provenance {
                parents: vec![],
                rules_applied: vec![
                    "input.capability_authorized".into(),
                    "input.payload_validated".into(),
                ],
                models_used: vec![],
                transform: Some(format!("adapter:{}", adapter_id.0)),
            }),
        })?;

        self.store.append(&artifact, self.schemas)?;
        Ok(artifact)
    }
}

fn input_subject(adapter_id: &AdapterId, capability_id: &str, external_subject: &str) -> SubjectId {
    SubjectId(format!(
        "input:{}:{}:{}:{}:{}:{}",
        adapter_id.0.len(),
        adapter_id.0,
        capability_id.len(),
        capability_id,
        external_subject.len(),
        external_subject,
    ))
}

fn input_stream_key(
    adapter_id: &AdapterId,
    capability_id: &str,
    external_reference: &str,
) -> String {
    format!(
        "input:{}:{}:{}:{}:{}:{}",
        adapter_id.0.len(),
        adapter_id.0,
        capability_id.len(),
        capability_id,
        external_reference.len(),
        external_reference,
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

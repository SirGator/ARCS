use crate::adapters::{
    AdapterId, AdapterRegistry, AdapterRegistryError, CapabilityContract, CapabilityRef,
    ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaRegistry, Source, SourceClass, SubjectId, Trust, TrustLevel,
};
use crate::store::SqliteArtifactStore;

use super::{
    CAPABILITY_AUTHORIZED_RULE, ObservationError, ObservationMessage, PAYLOAD_VALIDATED_RULE,
    RecordedObservation,
};

pub struct ObservationService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> ObservationService<'a> {
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

    pub fn ingest(
        &mut self,
        adapter_id: &AdapterId,
        message: ObservationMessage,
    ) -> Result<Artifact, ObservationError> {
        self.ingest_recorded(adapter_id, message)
            .map(RecordedObservation::into_artifact)
    }

    /// Nimmt eine Observation auf und gibt erst nach erfolgreichem Commit den
    /// für World-State-Updates benötigten Herkunftsnachweis zurück.
    pub fn ingest_recorded(
        &mut self,
        adapter_id: &AdapterId,
        message: ObservationMessage,
    ) -> Result<RecordedObservation, ObservationError> {
        if message.external_reference.trim().is_empty() {
            return Err(ObservationError::InvalidExternalReference);
        }

        let (registered, descriptor) = self
            .policy
            .authorized_capability(adapter_id, &message.capability_id)?;
        let capability = CapabilityRef {
            adapter_id: adapter_id.clone(),
            capability_id: message.capability_id.clone(),
        };
        let CapabilityContract::Observe { emits } = &descriptor.contract else {
            return Err(ObservationError::CapabilityIsNotObserve(capability));
        };
        if emits.len() != 1 {
            return Err(ObservationError::InvalidObserveSchemaCount {
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
            return Err(ObservationError::ExternalReferenceTooLarge {
                actual: reference_size,
                maximum: maximum_reference,
            });
        }
        let payload_size = serde_json::to_vec(&message.payload)?.len();
        if payload_size > maximum_payload {
            return Err(ObservationError::PayloadTooLarge {
                actual: payload_size,
                maximum: maximum_payload,
            });
        }

        self.schemas
            .validate(&schema_id, &message.payload)
            .map_err(ObservationError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| ObservationError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();

        let external_subject = message
            .external_subject
            .filter(|subject| !subject.trim().is_empty())
            .ok_or(ObservationError::MissingExternalSubject)?;
        let subject = observation_subject(adapter_id, &message.capability_id.0, &external_subject);
        let stream_key = observation_stream_key(&subject, &definition.id.0);

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
                    CAPABILITY_AUTHORIZED_RULE.into(),
                    PAYLOAD_VALIDATED_RULE.into(),
                ],
                models_used: vec![],
                transform: Some(format!("adapter:{}", adapter_id.0)),
            }),
        })?;

        let sequence = self.store.append_current(&artifact, self.schemas)?;
        Ok(RecordedObservation::from_committed(artifact, sequence))
    }
}

fn observation_subject(
    adapter_id: &AdapterId,
    capability_id: &str,
    external_subject: &str,
) -> SubjectId {
    SubjectId(format!(
        "observe:{}:{}:{}:{}:{}:{}",
        adapter_id.0.len(),
        adapter_id.0,
        capability_id.len(),
        capability_id,
        external_subject.len(),
        external_subject,
    ))
}

fn observation_stream_key(subject: &SubjectId, schema_id: &str) -> String {
    format!(
        "observe:{}:{}:{}:{}",
        subject.0.len(),
        subject.0,
        schema_id.len(),
        schema_id,
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

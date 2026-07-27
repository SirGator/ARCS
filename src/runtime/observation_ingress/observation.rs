//! Use Case für validierte, von außen gepushte Beobachtungen.

use super::envelope::{actor_type_for, trust_for};
use super::{AdapterGateway, AdapterGatewayError, AdapterSession};
use crate::adapters::observation::BoundarySubmission;
use crate::adapters::registration::{AdapterRegistryError, CapabilityRef};
use crate::core::{Actor, Artifact, Provenance, Source};

impl AdapterGateway<'_> {
    /// Nimmt eine untrusted Observe-Payload entgegen und speichert erst nach
    /// allen Prüfungen das vollständig vom Core umhüllte Artefakt.
    pub fn submit_boundary(
        &mut self,
        session: &AdapterSession,
        submission: BoundarySubmission,
    ) -> Result<Artifact, AdapterGatewayError> {
        if submission.external_reference.trim().is_empty() {
            return Err(AdapterGatewayError::InvalidBoundaryReference);
        }
        if session.gateway_instance_id != self.instance_id {
            return Err(AdapterGatewayError::InvalidAdapterSession);
        }

        let authenticated_adapter = self
            .adapter_sessions
            .get(&session.token)
            .cloned()
            .ok_or(AdapterGatewayError::InvalidAdapterSession)?;
        let (producer_class, assigned_trust, source_kind, maximum_payload, maximum_reference) = {
            let (registered, capability) = self
                .registry
                .authorized_capability(&authenticated_adapter, &submission.capability_id)?;
            if !capability.contract.is_observation()
                || !capability
                    .contract
                    .emitted_schemas()
                    .contains(&submission.schema_id)
            {
                return Err(AdapterGatewayError::CapabilityCannotPush(CapabilityRef {
                    adapter_id: authenticated_adapter,
                    capability_id: submission.capability_id,
                }));
            }
            (
                registered.grant().producer_class,
                registered.grant().assigned_trust,
                registered.grant().observation_source_kind.ok_or_else(|| {
                    AdapterGatewayError::AdapterRegistry(
                        AdapterRegistryError::MissingObservationSource,
                    )
                })?,
                registered.grant().max_payload_bytes,
                registered.grant().max_external_reference_bytes,
            )
        };

        let reference_size = submission.external_reference.len();
        if reference_size > maximum_reference {
            return Err(AdapterGatewayError::ExternalReferenceTooLarge {
                actual: reference_size,
                maximum: maximum_reference,
            });
        }
        let payload_size = serde_json::to_vec(&submission.payload)?.len();
        if payload_size > maximum_payload {
            return Err(AdapterGatewayError::PayloadTooLarge {
                actual: payload_size,
                maximum: maximum_payload,
            });
        }
        self.schemas
            .validate(&submission.schema_id, &submission.payload)
            .map_err(AdapterGatewayError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(&submission.schema_id)
            .ok_or_else(|| {
                AdapterGatewayError::MissingRegisteredSchema(submission.schema_id.clone())
            })?
            .clone();

        let generated = self.ids.next(&definition.artifact_type);
        let stream_key = observation_stream_key(
            &authenticated_adapter.0,
            &submission.subject.0,
            &definition.id.0,
        );
        let artifact = Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: actor_type_for(producer_class),
                id: authenticated_adapter.0.clone(),
            },
            source: Source {
                kind: source_kind,
                reference: submission.external_reference,
            },
            trust: trust_for(producer_class, assigned_trust),
            stream_key,
            subject: Some(submission.subject),
            tags: vec![
                format!("adapter:{}", authenticated_adapter.0),
                format!("capability:{}", submission.capability_id.0),
            ],
            payload: submission.payload,
            provenance: Some(Provenance {
                parents: vec![],
                rules_applied: vec![
                    "adapter_gateway.capability_authorized".into(),
                    "adapter_gateway.payload_schema_validated".into(),
                ],
                models_used: vec![],
                transform: Some(format!("adapter:{}", authenticated_adapter.0)),
            }),
        };

        self.store.append_current(&artifact, self.schemas)?;
        Ok(artifact)
    }
}

/// Bildet eine kollisionsfreie, über einzelne Observationen hinweg stabile
/// Stream-Identität. Längenpräfixe trennen auch frei benennbare Subjects
/// eindeutig voneinander, ohne deren adapterspezifische Syntax einzuschränken.
fn observation_stream_key(adapter_id: &str, subject: &str, schema_id: &str) -> String {
    format!(
        "observe:{}:{adapter_id}:{}:{subject}:{}:{schema_id}",
        adapter_id.len(),
        subject.len(),
        schema_id.len()
    )
}

#[cfg(test)]
mod tests;

//! Use Case für korrelierte, gezielt angeforderte Umgebungsdaten.

use super::envelope::{actor_type_for, trust_for};
use super::{AdapterGateway, AdapterGatewayError};
use crate::adapters::data::DataInvocation;
use crate::adapters::registration::{AdapterRegistryError, CapabilityRef, ProducerClass};
use crate::core::{Actor, Artifact, Provenance, SchemaId, Source, VersionId};
use crate::store::relation_kinds;

impl AdapterGateway<'_> {
    /// Ruft genau eine freigeschaltete Data-Capability für ein persistiertes
    /// Request-Artifact auf.
    ///
    /// Der Adapter liefert ausschließlich Referenz und Payload. Identität,
    /// Subject, Trust, Provenance und Relationen werden vom Core festgelegt.
    /// Damit kann eine Adapterantwort weder einen fremden Request erfüllen
    /// noch ihren eigenen Autoritätsumschlag behaupten.
    pub fn request_data(
        &mut self,
        capability_ref: &CapabilityRef,
        request_version_id: &VersionId,
        response_schema_id: &SchemaId,
    ) -> Result<Artifact, AdapterGatewayError> {
        let completion_key = (
            capability_ref.clone(),
            request_version_id.clone(),
            response_schema_id.clone(),
        );
        if self.completed_data_invocations.contains(&completion_key) {
            return Err(AdapterGatewayError::InvocationAlreadyCompleted {
                capability: capability_ref.clone(),
                input: request_version_id.clone(),
                response_schema: response_schema_id.clone(),
            });
        }

        let request = self
            .store
            .get(request_version_id)?
            .ok_or_else(|| AdapterGatewayError::MissingInputArtifact(request_version_id.clone()))?;
        let subject = request.subject.clone().ok_or_else(|| {
            AdapterGatewayError::MissingRequestSubject(request_version_id.clone())
        })?;

        // Alle Grant-Werte werden vor dem externen Aufruf kopiert. Dadurch
        // bleibt der Registry-Borrow kurz und der Adapter sieht nie die
        // Core-seitige Registry oder den Betreiber-Grant.
        let (
            authorized_capability,
            producer_class,
            assigned_trust,
            source_kind,
            maximum_payload,
            maximum_reference,
        ) = {
            let (registered, capability) = self
                .registry
                .authorized_capability(&capability_ref.adapter_id, &capability_ref.capability_id)?;
            if !capability.contract.is_data() {
                return Err(AdapterGatewayError::NotDataAdapter(
                    capability_ref.adapter_id.clone(),
                ));
            }
            if registered.grant().producer_class == ProducerClass::Model {
                return Err(AdapterGatewayError::DataProducerMustNotBeModel(
                    capability_ref.adapter_id.clone(),
                ));
            }
            if !capability
                .contract
                .accepted_schemas()
                .contains(&request.schema_id)
            {
                return Err(AdapterGatewayError::InputSchemaNotAccepted {
                    capability: capability_ref.clone(),
                    schema: request.schema_id.clone(),
                });
            }
            if !capability
                .contract
                .emitted_schemas()
                .contains(response_schema_id)
            {
                return Err(AdapterGatewayError::UndeclaredOutputSchema {
                    capability: capability_ref.capability_id.clone(),
                    schema: response_schema_id.clone(),
                });
            }

            (
                CapabilityRef {
                    adapter_id: registered.manifest().adapter_id.clone(),
                    capability_id: capability.id.clone(),
                },
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

        // Das Zielschema wird ebenfalls vor dem Adapteraufruf aufgelöst.
        // Ein lokaler Konfigurationsfehler löst dadurch keine externe Abfrage aus.
        let definition = self
            .schemas
            .get(response_schema_id)
            .ok_or_else(|| {
                AdapterGatewayError::MissingRegisteredSchema(response_schema_id.clone())
            })?
            .clone();
        let invocation_id = data_invocation_id(
            &authorized_capability,
            request_version_id,
            response_schema_id,
        );
        let invocation = DataInvocation {
            invocation_id: invocation_id.clone(),
            capability: authorized_capability,
            request_version_id: request.version_id.clone(),
            request_schema_id: request.schema_id.clone(),
            subject: subject.clone(),
            request_payload: request.payload.clone(),
            response_schema_id: response_schema_id.clone(),
            max_response_bytes: maximum_payload,
        };

        let endpoint = self
            .data_endpoints
            .get(&capability_ref.adapter_id)
            .ok_or_else(|| {
                AdapterGatewayError::MissingDataEndpoint(capability_ref.adapter_id.clone())
            })?;
        let response = endpoint.fetch(&invocation)?;

        if response.invocation_id != invocation_id {
            return Err(AdapterGatewayError::InvocationResponseMismatch);
        }
        if response.external_reference.trim().is_empty() {
            return Err(AdapterGatewayError::InvalidBoundaryReference);
        }
        let reference_size = response.external_reference.len();
        if reference_size > maximum_reference {
            return Err(AdapterGatewayError::ExternalReferenceTooLarge {
                actual: reference_size,
                maximum: maximum_reference,
            });
        }
        let payload_size = serde_json::to_vec(&response.payload)?.len();
        if payload_size > maximum_payload {
            return Err(AdapterGatewayError::PayloadTooLarge {
                actual: payload_size,
                maximum: maximum_payload,
            });
        }
        self.schemas
            .validate(response_schema_id, &response.payload)
            .map_err(AdapterGatewayError::InvalidPayload)?;

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
                actor_type: actor_type_for(producer_class),
                id: capability_ref.adapter_id.0.clone(),
            },
            source: Source {
                kind: source_kind,
                reference: response.external_reference,
            },
            trust: trust_for(producer_class, assigned_trust),
            // Der bereits validierte Request-Stream bleibt unverändert. Ein
            // Präfix würde seine feste Envelope-Grenze nach dem externen
            // Fetch überschreiten können.
            stream_key: request.stream_key.clone(),
            subject: Some(subject),
            tags: vec![
                format!("adapter:{}", capability_ref.adapter_id.0),
                format!("capability:{}", capability_ref.capability_id.0),
                format!("request:{}", request.version_id.0),
            ],
            payload: response.payload,
            provenance: Some(Provenance {
                parents: vec![request.version_id.0.clone()],
                rules_applied: vec![
                    "adapter_gateway.data_capability_authorized".into(),
                    "adapter_gateway.data_response_correlated".into(),
                    "adapter_gateway.payload_schema_validated".into(),
                ],
                models_used: vec![],
                transform: Some(format!("data_adapter:{}", capability_ref.adapter_id.0)),
            }),
        };

        // Artifact, Current-State-Zeiger und beide Korrelationen werden
        // gemeinsam committed. Bei einem Fehler bleibt kein Teilzustand zurück.
        self.store.append_current_related(
            &artifact,
            self.schemas,
            &[
                (request.version_id.clone(), relation_kinds::fulfills()),
                (request.version_id, relation_kinds::caused_by()),
            ],
        )?;
        self.completed_data_invocations.insert(completion_key);
        Ok(artifact)
    }
}

/// Stabile, ausschließlich vom Core abgeleitete Korrelations-ID.
///
/// Längenpräfixe verhindern Mehrdeutigkeiten, selbst wenn eine Versions-ID
/// Trennzeichen enthält. Dieselbe fachliche Invocation erhält dadurch bei
/// einem Transport-Retry wieder dieselbe ID.
fn data_invocation_id(
    capability: &CapabilityRef,
    request: &VersionId,
    response_schema: &SchemaId,
) -> String {
    format!(
        "data:{}:{}:{}:{}:{}:{}:{}:{}",
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

#[cfg(test)]
mod tests;

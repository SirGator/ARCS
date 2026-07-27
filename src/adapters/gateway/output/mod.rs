//! Use Case für korrelierte, idempotente Ausgaben an externe Senken.

use super::envelope::{actor_type_for, trust_for};
use super::{AdapterGateway, AdapterGatewayError};
use crate::adapters::output::OutputInvocation;
use crate::adapters::registration::{CapabilityRef, ProducerClass};
use crate::core::{Actor, Artifact, Provenance, SchemaId, Source, SourceKind, VersionId};
use crate::store::relation_kinds;

impl AdapterGateway<'_> {
    /// Liefert genau eine bereits persistierte Artifact-Version über eine
    /// vollständig qualifizierte Output-Fähigkeit aus.
    ///
    /// Der Core erzeugt Invocation-ID und Result-Envelope selbst. Dadurch kann
    /// der Adapter weder einen fremden Auftrag bestätigen noch autoritative
    /// Metadaten oder Relationen einschleusen. Ein Retry verwendet dieselbe
    /// Invocation-ID; die Installation ist deshalb nur mit einem explizit
    /// idempotenten Output-Vertrag zulässig.
    pub fn deliver_output(
        &mut self,
        capability: &CapabilityRef,
        artifact_version_id: &VersionId,
        result_schema_id: &SchemaId,
    ) -> Result<Artifact, AdapterGatewayError> {
        let input = self.store.get(artifact_version_id)?.ok_or_else(|| {
            AdapterGatewayError::MissingInputArtifact(artifact_version_id.clone())
        })?;

        let (authorized_capability, assigned_trust, maximum_payload, maximum_reference) = {
            let (registered, descriptor) = self
                .registry
                .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
            if !descriptor.contract.is_output() {
                return Err(AdapterGatewayError::NotOutputAdapter(
                    capability.adapter_id.clone(),
                ));
            }
            if registered.grant().producer_class != ProducerClass::Executor {
                return Err(AdapterGatewayError::OutputProducerMustBeExecutor(
                    capability.adapter_id.clone(),
                ));
            }
            if !descriptor
                .contract
                .accepted_schemas()
                .contains(&input.schema_id)
            {
                return Err(AdapterGatewayError::InputSchemaNotAccepted {
                    capability: capability.clone(),
                    schema: input.schema_id.clone(),
                });
            }
            if !descriptor
                .contract
                .emitted_schemas()
                .contains(result_schema_id)
            {
                return Err(AdapterGatewayError::UndeclaredOutputSchema {
                    capability: capability.capability_id.clone(),
                    schema: result_schema_id.clone(),
                });
            }
            (
                CapabilityRef {
                    adapter_id: registered.manifest().adapter_id.clone(),
                    capability_id: descriptor.id.clone(),
                },
                registered.grant().assigned_trust,
                registered.grant().max_payload_bytes,
                registered.grant().max_external_reference_bytes,
            )
        };

        let completion_key = (
            capability.clone(),
            artifact_version_id.clone(),
            result_schema_id.clone(),
        );
        if self.completed_output_invocations.contains(&completion_key) {
            return Err(AdapterGatewayError::InvocationAlreadyCompleted {
                capability: capability.clone(),
                input: artifact_version_id.clone(),
                response_schema: result_schema_id.clone(),
            });
        }

        let invocation_id = output_invocation_id(
            &authorized_capability,
            artifact_version_id,
            result_schema_id,
        );
        let invocation = OutputInvocation {
            invocation_id: invocation_id.clone(),
            capability: authorized_capability,
            artifact_version_id: input.version_id.clone(),
            artifact_schema_id: input.schema_id.clone(),
            subject: input.subject.clone(),
            payload: input.payload.clone(),
            result_schema_id: result_schema_id.clone(),
        };
        let endpoint = self
            .output_endpoints
            .get(&capability.adapter_id)
            .ok_or_else(|| {
                AdapterGatewayError::MissingOutputEndpoint(capability.adapter_id.clone())
            })?;
        let response = endpoint.deliver(&invocation)?;

        // Erst die Core-Korrelation prüfen. Eine formal gültige Payload darf
        // niemals als Bestätigung eines anderen Auftrags übernommen werden.
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
        let payload_size = serde_json::to_vec(&response.result_payload)?.len();
        if payload_size > maximum_payload {
            return Err(AdapterGatewayError::PayloadTooLarge {
                actual: payload_size,
                maximum: maximum_payload,
            });
        }
        self.schemas
            .validate(result_schema_id, &response.result_payload)
            .map_err(AdapterGatewayError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(result_schema_id)
            .ok_or_else(|| AdapterGatewayError::MissingRegisteredSchema(result_schema_id.clone()))?
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
                actor_type: actor_type_for(ProducerClass::Executor),
                id: capability.adapter_id.0.clone(),
            },
            source: Source {
                // Der Umschlag ist Core-korreliert, sein Inhalt stammt dennoch
                // aus der Empfangsbestätigung des externen Executors.
                kind: SourceKind::External,
                reference: response.external_reference,
            },
            trust: trust_for(ProducerClass::Executor, assigned_trust),
            stream_key: input.stream_key.clone(),
            subject: input.subject.clone(),
            tags: vec![
                format!("adapter:{}", capability.adapter_id.0),
                format!("capability:{}", capability.capability_id.0),
                "output_result".into(),
            ],
            payload: response.result_payload,
            provenance: Some(Provenance {
                parents: vec![input.version_id.0.clone()],
                rules_applied: vec![
                    "adapter_gateway.output_capability_authorized".into(),
                    "adapter_gateway.output_response_correlated".into(),
                    "adapter_gateway.output_result_schema_validated".into(),
                ],
                models_used: vec![],
                transform: Some(format!("output_adapter:{}", capability.adapter_id.0)),
            }),
        };

        self.store.append_related(
            &artifact,
            self.schemas,
            &[(input.version_id, relation_kinds::result_of())],
        )?;
        // Erst nach dem atomaren Store-Commit gilt der Auftrag für diese
        // Runtime als abgeschlossen. Scheitert der Commit, ist ein Retry dank
        // identischer Invocation-ID für den Adapter wirkungsneutral.
        self.completed_output_invocations.insert(completion_key);
        Ok(artifact)
    }
}

fn output_invocation_id(
    capability: &CapabilityRef,
    input: &VersionId,
    result_schema: &SchemaId,
) -> String {
    format!(
        "output:{}:{}:{}:{}:{}:{}:{}:{}",
        capability.adapter_id.0.len(),
        capability.adapter_id.0,
        capability.capability_id.0.len(),
        capability.capability_id.0,
        input.0.len(),
        input.0,
        result_schema.0.len(),
        result_schema.0
    )
}

#[cfg(test)]
mod tests;

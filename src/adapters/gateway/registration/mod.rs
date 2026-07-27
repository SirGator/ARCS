//! Use Cases zum atomaren Installieren externer Adapter.

use super::reasoning::validate_reasoning_output_schemas;
use super::{AdapterGateway, AdapterGatewayError, AdapterSession};
use crate::adapters::data::DataAdapter;
use crate::adapters::output::OutputAdapter;
use crate::adapters::reasoning::ReasoningAdapter;
use crate::adapters::registration::{
    AdapterGrant, AdapterId, AdapterManifest, CapabilityContract, CapabilityRef, ProducerClass,
};
use crate::core::SchemaRegistry;

impl AdapterGateway<'_> {
    /// Installiert Manifest, ausschließlich Core-seitigen Grant und optionale
    /// externe Schemas als eine atomare Einheit.
    pub fn register_adapter(
        &mut self,
        manifest: AdapterManifest,
        grant: AdapterGrant,
        schema_documents: &[&str],
    ) -> Result<AdapterSession, AdapterGatewayError> {
        ensure_endpoint_contracts(&manifest, &grant, |_| false)?;
        let staged = self.stage_schemas(schema_documents)?;
        self.registry
            .validate_registration(&manifest, &grant, &staged)?;
        self.store.bind_schemas(&staged)?;

        let adapter_id = manifest.adapter_id.clone();
        let session = self.issue_session(adapter_id)?;
        *self.schemas = staged;
        self.registry.insert_validated(manifest, grant);
        Ok(session)
    }

    /// Installiert zusätzlich den internen Port zu einem externen
    /// Reasoning-Prozess.
    pub fn register_reasoning_adapter(
        &mut self,
        adapter: Box<dyn ReasoningAdapter>,
        grant: AdapterGrant,
        schema_documents: &[&str],
    ) -> Result<AdapterSession, AdapterGatewayError> {
        let manifest = adapter.manifest().clone();
        ensure_endpoint_contracts(&manifest, &grant, CapabilityContract::is_reasoning)?;
        if self.reasoning_endpoints.contains_key(&manifest.adapter_id) {
            return Err(AdapterGatewayError::DuplicateReasoningEndpoint(
                manifest.adapter_id,
            ));
        }
        if grant.producer_class != ProducerClass::Model {
            return Err(AdapterGatewayError::ReasoningProducerMustBeModel(
                manifest.adapter_id,
            ));
        }
        let has_enabled_reasoning = grant.enabled_capabilities.iter().any(|id| {
            manifest
                .capability(id)
                .is_some_and(|capability| capability.contract.is_reasoning())
        });
        if !has_enabled_reasoning {
            return Err(AdapterGatewayError::NotReasoningAdapter(
                manifest.adapter_id,
            ));
        }

        let staged = self.stage_schemas(schema_documents)?;
        self.registry
            .validate_registration(&manifest, &grant, &staged)?;
        validate_reasoning_output_schemas(&manifest, &grant, &staged)?;
        self.store.bind_schemas(&staged)?;

        let adapter_id = manifest.adapter_id.clone();
        let session = self.issue_session(adapter_id.clone())?;
        *self.schemas = staged;
        self.registry.insert_validated(manifest, grant);
        self.reasoning_endpoints.insert(adapter_id, adapter);
        Ok(session)
    }

    /// Installiert einen korreliert aufrufbaren Data-Adapter.
    pub fn register_data_adapter(
        &mut self,
        adapter: Box<dyn DataAdapter>,
        grant: AdapterGrant,
        schema_documents: &[&str],
    ) -> Result<AdapterSession, AdapterGatewayError> {
        let manifest = adapter.manifest().clone();
        ensure_endpoint_contracts(&manifest, &grant, CapabilityContract::is_data)?;
        if self.data_endpoints.contains_key(&manifest.adapter_id) {
            return Err(AdapterGatewayError::DuplicateDataEndpoint(
                manifest.adapter_id,
            ));
        }
        if grant.producer_class == ProducerClass::Model {
            return Err(AdapterGatewayError::DataProducerMustNotBeModel(
                manifest.adapter_id,
            ));
        }
        let has_enabled_data = grant.enabled_capabilities.iter().any(|id| {
            manifest
                .capability(id)
                .is_some_and(|capability| capability.contract.is_data())
        });
        if !has_enabled_data {
            return Err(AdapterGatewayError::NotDataAdapter(manifest.adapter_id));
        }

        let staged = self.stage_schemas(schema_documents)?;
        self.registry
            .validate_registration(&manifest, &grant, &staged)?;
        self.store.bind_schemas(&staged)?;
        let adapter_id = manifest.adapter_id.clone();
        let session = self.issue_session(adapter_id.clone())?;
        *self.schemas = staged;
        self.registry.insert_validated(manifest, grant);
        self.data_endpoints.insert(adapter_id, adapter);
        Ok(session)
    }

    /// Installiert einen explizit aufrufbaren Output-Adapter.
    pub fn register_output_adapter(
        &mut self,
        adapter: Box<dyn OutputAdapter>,
        grant: AdapterGrant,
        schema_documents: &[&str],
    ) -> Result<AdapterSession, AdapterGatewayError> {
        let manifest = adapter.manifest().clone();
        ensure_endpoint_contracts(&manifest, &grant, CapabilityContract::is_output)?;
        if self.output_endpoints.contains_key(&manifest.adapter_id) {
            return Err(AdapterGatewayError::DuplicateOutputEndpoint(
                manifest.adapter_id,
            ));
        }
        if grant.producer_class != ProducerClass::Executor {
            return Err(AdapterGatewayError::OutputProducerMustBeExecutor(
                manifest.adapter_id,
            ));
        }
        let has_enabled_output = grant.enabled_capabilities.iter().any(|id| {
            manifest
                .capability(id)
                .is_some_and(|capability| capability.contract.is_output())
        });
        if !has_enabled_output {
            return Err(AdapterGatewayError::NotOutputAdapter(manifest.adapter_id));
        }

        let staged = self.stage_schemas(schema_documents)?;
        self.registry
            .validate_registration(&manifest, &grant, &staged)?;
        self.store.bind_schemas(&staged)?;
        let adapter_id = manifest.adapter_id.clone();
        let session = self.issue_session(adapter_id.clone())?;
        *self.schemas = staged;
        self.registry.insert_validated(manifest, grant);
        self.output_endpoints.insert(adapter_id, adapter);
        Ok(session)
    }

    fn issue_session(
        &mut self,
        adapter_id: AdapterId,
    ) -> Result<AdapterSession, AdapterGatewayError> {
        let token = self.next_session_token;
        self.next_session_token = self
            .next_session_token
            .checked_add(1)
            .ok_or(AdapterGatewayError::SessionTokenExhausted)?;
        self.adapter_sessions.insert(token, adapter_id);
        Ok(AdapterSession {
            gateway_instance_id: self.instance_id,
            token,
        })
    }

    fn stage_schemas(
        &self,
        schema_documents: &[&str],
    ) -> Result<SchemaRegistry, AdapterGatewayError> {
        let mut staged = (*self.schemas).clone();
        for document in schema_documents {
            staged.register_json(document)?;
        }
        Ok(staged)
    }
}

/// Verhindert, dass ein korrelierter Port ohne den zugehörigen Endpoint-Slice
/// als enabled in die Registry gelangt.
///
/// Observe, Transform und das noch nicht dispatchte Act dürfen neben dem
/// erlaubten Endpoint-Typ deklariert sein. Data, Reasoning und Output benötigen
/// dagegen jeweils das passende Trait-Objekt und dessen zusätzliche Prüfungen.
fn ensure_endpoint_contracts(
    manifest: &AdapterManifest,
    grant: &AdapterGrant,
    allowed: fn(&CapabilityContract) -> bool,
) -> Result<(), AdapterGatewayError> {
    for capability_id in &grant.enabled_capabilities {
        let Some(capability) = manifest.capability(capability_id) else {
            // Die Registry liefert anschließend den präzisen Unknown-Fehler.
            continue;
        };
        let needs_endpoint = capability.contract.is_data()
            || capability.contract.is_reasoning()
            || capability.contract.is_output();
        if needs_endpoint && !allowed(&capability.contract) {
            return Err(AdapterGatewayError::CapabilityRequiresDedicatedEndpoint(
                CapabilityRef {
                    adapter_id: manifest.adapter_id.clone(),
                    capability_id: capability_id.clone(),
                },
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

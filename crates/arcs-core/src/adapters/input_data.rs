//! Kontextgebundener, beidseitig autorisierter Transport von Binäreingaben.
use super::data::{DataAdapter, DataError, DataService};
use super::{AdapterRegistry, AdapterRegistryError, CapabilityRef};
use crate::core::{SchemaRegistry, validate_artifact};
use crate::reasoning::ReasoningContextItem;
use crate::store::{SqliteArtifactStore, StoreError};
#[cfg(test)]
mod tests;
pub use arcs_adapter_protocol::{
    InputDataDescriptor, InputDataLimits, InputDataSelection, ReceivedInputData,
};
#[derive(Debug)]
pub enum InputDataError {
    Invalid(String),
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Data(DataError),
}
impl From<AdapterRegistryError> for InputDataError {
    fn from(e: AdapterRegistryError) -> Self {
        Self::Authorization(e)
    }
}
impl From<StoreError> for InputDataError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<DataError> for InputDataError {
    fn from(e: DataError) -> Self {
        Self::Data(e)
    }
}
fn invalid(message: &str) -> InputDataError {
    InputDataError::Invalid(message.into())
}
pub struct InputDataService<'a> {
    registry: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
}
impl<'a> InputDataService<'a> {
    pub fn new(
        registry: &'a AdapterRegistry,
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
    ) -> Self {
        Self {
            registry,
            schemas,
            store,
        }
    }
    pub fn prepare(
        &self,
        target: &CapabilityRef,
        context: &[ReasoningContextItem],
        selections: &[InputDataSelection],
    ) -> Result<Vec<InputDataDescriptor>, InputDataError> {
        if selections.is_empty() {
            return Ok(vec![]);
        }
        let (receiver, cap) = self
            .registry
            .authorized_capability(&target.adapter_id, &target.capability_id)?;
        let limits = receiver
            .grant()
            .input_data_limits
            .as_ref()
            .filter(|limits| limits.is_valid())
            .ok_or_else(|| invalid("binary input is not granted"))?;
        if !cap.contract.is_reasoning()
            || !receiver
                .grant()
                .granted_permissions
                .iter()
                .any(|permission| permission == "data.receive")
            || selections.len() > limits.max_objects
        {
            return Err(invalid(
                "receiver role, permission or object limit mismatch",
            ));
        }
        let mut names = std::collections::HashSet::new();
        let mut total = 0u64;
        let mut inputs = Vec::new();
        for selection in selections {
            let item = context
                .iter()
                .find(|item| item.version_id == selection.source_version_id)
                .ok_or_else(|| invalid("binary input is outside selected context"))?;
            let artifact = self
                .store
                .get(&selection.source_version_id)?
                .ok_or_else(|| invalid("binary source artifact is missing"))?;
            validate_artifact(&artifact, self.schemas)
                .map_err(|_| invalid("binary source artifact is invalid"))?;
            if artifact.created_by.id != selection.source.adapter_id.0
                || !artifact
                    .tags
                    .contains(&format!("capability:{}", selection.source.capability_id.0))
            {
                return Err(invalid("binary source differs from persisted provenance"));
            }
            let (source, descriptor) = self.registry.authorized_capability(
                &selection.source.adapter_id,
                &selection.source.capability_id,
            )?;
            if source.manifest().adapter_id != selection.source.adapter_id
                || !descriptor
                    .contract
                    .emitted_schemas()
                    .contains(&artifact.schema_id)
            {
                return Err(invalid(
                    "binary source schema does not match its capability",
                ));
            }
            let value = item
                .payload
                .pointer(&selection.reference_pointer)
                .ok_or_else(|| invalid("binary reference field was not selected"))?;
            if artifact.payload.pointer(&selection.reference_pointer) != Some(value) {
                return Err(invalid("binary context differs from stored source"));
            }
            let reference = serde_json::from_value(value.clone())
                .map_err(|_| invalid("selected field is not a data reference"))?;
            let input = InputDataDescriptor {
                selection: selection.clone(),
                reference,
            };
            if !input.is_valid() || !names.insert(&selection.name) {
                return Err(invalid("invalid binary input descriptor or duplicate name"));
            }
            total = total
                .checked_add(input.reference.size_bytes)
                .ok_or_else(|| invalid("binary input size overflow"))?;
            if total > limits.max_total_bytes {
                return Err(invalid("binary inputs exceed receiver byte limit"));
            }
            DataService::new(self.registry).authorize(&selection.source, &input.reference)?;
            inputs.push(input);
        }
        inputs.sort_by(|a, b| a.selection.name.cmp(&b.selection.name));
        Ok(inputs)
    }
    pub fn read(
        &self,
        inputs: &[InputDataDescriptor],
        sources: &[&dyn DataAdapter],
    ) -> Result<Vec<ReceivedInputData>, InputDataError> {
        let mut received = Vec::new();
        for input in inputs {
            let endpoint = sources
                .iter()
                .find(|endpoint| {
                    endpoint.manifest().adapter_id == input.selection.source.adapter_id
                })
                .ok_or_else(|| invalid("binary source endpoint is missing"))?;
            let mut bytes = Vec::new();
            DataService::new(self.registry).copy(
                *endpoint,
                &input.selection.source,
                &input.reference,
                &mut bytes,
            )?;
            received.push(ReceivedInputData {
                descriptor: input.clone(),
                bytes,
            });
        }
        Ok(received)
    }
}

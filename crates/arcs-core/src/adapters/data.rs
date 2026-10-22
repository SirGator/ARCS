//! Autorisierter Datenabruf mit begrenztem Speicherbedarf.

use std::io::Write;

use super::{
    AdapterCallError, AdapterManifest, AdapterRegistry, AdapterRegistryError, CapabilityRef,
};
pub use arcs_adapter_protocol::{DataChunk, DataLimits, DataRead, DataReference};

#[cfg(test)]
mod tests;

pub trait DataAdapter {
    fn manifest(&self) -> &AdapterManifest;
    fn read_chunk(&self, request: &DataRead) -> Result<DataChunk, AdapterCallError>;
}

#[derive(Debug)]
pub enum DataError {
    Authorization(AdapterRegistryError),
    NotGranted,
    InvalidReference,
    ObjectTooLarge,
    EndpointMismatch,
    UnsupportedCapability,
    Adapter(AdapterCallError),
    InvalidChunk,
    Write(std::io::Error),
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "data transfer: {self:?}")
    }
}
impl std::error::Error for DataError {}

pub struct DataService<'a> {
    registry: &'a AdapterRegistry,
}

impl<'a> DataService<'a> {
    pub fn new(registry: &'a AdapterRegistry) -> Self {
        Self { registry }
    }

    /// Schreibt geprüfte Blöcke fortlaufend. Bei Fehlern können bereits Bytes
    /// geschrieben sein; ein Dateiexport sollte eine temporäre Datei verwenden.
    pub fn copy(
        &self,
        adapter: &impl DataAdapter,
        capability: &CapabilityRef,
        reference: &DataReference,
        writer: &mut impl Write,
    ) -> Result<u64, DataError> {
        let (registered, descriptor) = self
            .registry
            .authorized_capability(&capability.adapter_id, &capability.capability_id)
            .map_err(DataError::Authorization)?;
        if registered.manifest() != adapter.manifest() {
            return Err(DataError::EndpointMismatch);
        }
        if !descriptor.contract.is_observation() && !descriptor.contract.is_request() {
            return Err(DataError::UnsupportedCapability);
        }
        let grant = registered.grant();
        let limits = grant
            .data_limits
            .filter(|limits| limits.is_valid())
            .filter(|_| {
                grant
                    .granted_permissions
                    .iter()
                    .any(|permission| permission == "data.read")
            })
            .ok_or(DataError::NotGranted)?;
        if !reference.is_valid() {
            return Err(DataError::InvalidReference);
        }
        if reference.size_bytes > limits.max_object_bytes {
            return Err(DataError::ObjectTooLarge);
        }
        let mut offset = 0;
        let mut first = true;
        while first || offset < reference.size_bytes {
            first = false;
            let request = DataRead {
                capability: capability.clone(),
                reference: reference.clone(),
                offset,
                max_bytes: limits.max_chunk_bytes,
            };
            let chunk = adapter.read_chunk(&request).map_err(DataError::Adapter)?;
            if !chunk.matches(&request) {
                return Err(DataError::InvalidChunk);
            }
            writer.write_all(&chunk.bytes).map_err(DataError::Write)?;
            offset += chunk.bytes.len() as u64;
        }
        Ok(offset)
    }
}

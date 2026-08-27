use crate::adapters::{AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation};
use crate::store::StoreError;

#[derive(Debug)]
pub enum InputError {
    Authorization(AdapterRegistryError),
    InvalidExternalReference,
    CapabilityIsNotInput(CapabilityRef),
    InvalidInputSchemaCount {
        capability: CapabilityRef,
        actual: usize,
    },
    IdentityConflict(String),
    MissingExternalSubject,
    ExternalReferenceTooLarge {
        actual: usize,
        maximum: usize,
    },
    PayloadTooLarge {
        actual: usize,
        maximum: usize,
    },
    MissingRegisteredSchema(SchemaId),
    InvalidPayload(Vec<SchemaViolation>),
    Factory(ArtifactFactoryError),
    Store(StoreError),
    Serialization(serde_json::Error),
}

impl From<AdapterRegistryError> for InputError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for InputError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for InputError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for InputError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

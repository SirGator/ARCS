use crate::adapters::{AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation};
use crate::store::StoreError;

#[derive(Debug)]
pub enum ObservationIngressError {
    Authorization(AdapterRegistryError),

    InvalidExternalReference,

    CapabilityIsNotObserve(CapabilityRef),

    InvalidObserveSchemaCount {
        capability: CapabilityRef,
        actual: usize,
    },

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

impl From<AdapterRegistryError> for ObservationIngressError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for ObservationIngressError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for ObservationIngressError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for ObservationIngressError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

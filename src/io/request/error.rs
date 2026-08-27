use crate::adapters::{
    AdapterCallError, AdapterId, AdapterRegistryError, CapabilityId, CapabilityRef,
};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation, VersionId};
use crate::runtime::InvocationError;
use crate::store::StoreError;

/// Fehler ausschließlich an der Grenze für aktiv angeforderte Daten.
#[derive(Debug)]
pub enum RequestError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    AdapterCall(AdapterCallError),
    Invocation(InvocationError),
    CapabilityIsNotRequest(CapabilityRef),
    ModelMustNotServeRequests(AdapterId),
    MissingInputArtifact(VersionId),
    MissingRequestSubject(VersionId),
    InputSchemaNotAccepted {
        capability: CapabilityRef,
        schema: SchemaId,
    },
    UndeclaredResponseSchema {
        capability: CapabilityId,
        schema: SchemaId,
    },
    MissingRegisteredSchema(SchemaId),
    MissingSourceKind,
    InvocationResponseMismatch,
    InvalidExternalReference,
    ExternalReferenceTooLarge {
        actual: usize,
        maximum: usize,
    },
    PayloadTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidPayload(Vec<SchemaViolation>),
}

impl From<AdapterRegistryError> for RequestError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for RequestError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for RequestError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for RequestError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

impl From<AdapterCallError> for RequestError {
    fn from(value: AdapterCallError) -> Self {
        Self::AdapterCall(value)
    }
}

impl From<InvocationError> for RequestError {
    fn from(value: InvocationError) -> Self {
        Self::Invocation(value)
    }
}

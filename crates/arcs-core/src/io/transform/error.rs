use crate::adapters::{AdapterCallError, AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation, ValidationError, VersionId};
use crate::runtime::InvocationError;
use crate::store::StoreError;

#[derive(Debug)]
pub enum TransformError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    Invocation(InvocationError),
    AdapterCall(AdapterCallError),
    EndpointMismatch,
    CapabilityIsNotTransform(CapabilityRef),
    ModelMustUseReasoning,
    MissingInput(VersionId),
    InvalidInput(ValidationError),
    InputSchemaNotAccepted(SchemaId),
    UndeclaredOutputSchema(SchemaId),
    MissingSchema(SchemaId),
    ControlOutputForbidden(SchemaId),
    CachedResultNotPermitted,
    InputTooLarge,
    OutputTooLarge,
    InvocationResponseMismatch,
    InvalidExternalReference,
    InvalidOutput(Vec<SchemaViolation>),
}

impl From<AdapterRegistryError> for TransformError {
    fn from(error: AdapterRegistryError) -> Self {
        Self::Authorization(error)
    }
}
impl From<StoreError> for TransformError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
impl From<ArtifactFactoryError> for TransformError {
    fn from(error: ArtifactFactoryError) -> Self {
        Self::Factory(error)
    }
}
impl From<serde_json::Error> for TransformError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}
impl From<InvocationError> for TransformError {
    fn from(error: InvocationError) -> Self {
        Self::Invocation(error)
    }
}
impl From<AdapterCallError> for TransformError {
    fn from(error: AdapterCallError) -> Self {
        Self::AdapterCall(error)
    }
}

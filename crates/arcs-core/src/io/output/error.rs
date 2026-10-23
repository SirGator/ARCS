use crate::adapters::{AdapterCallError, AdapterRegistryError};
use crate::core::{ArtifactFactoryError, ValidationError, VersionId};
use crate::runtime::InvocationError;
use crate::store::StoreError;

#[derive(Debug)]
pub enum OutputError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    Invocation(InvocationError),
    AdapterCall(AdapterCallError),
    MissingArtifact(VersionId),
    InvalidArtifact(ValidationError),
    Gate(&'static str),
    ContractMismatch,
    ExecutorRequired,
    ControlResultForbidden,
    InputTooLarge,
    InvalidResponse,
    ResultTooLarge,
    CachedResultNotPermitted,
}
impl From<AdapterRegistryError> for OutputError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}
impl From<StoreError> for OutputError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<ArtifactFactoryError> for OutputError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}
impl From<serde_json::Error> for OutputError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}
impl From<InvocationError> for OutputError {
    fn from(value: InvocationError) -> Self {
        Self::Invocation(value)
    }
}
impl From<AdapterCallError> for OutputError {
    fn from(value: AdapterCallError) -> Self {
        Self::AdapterCall(value)
    }
}

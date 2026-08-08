use crate::core::{ArtifactFactoryError, SchemaId, VersionId};
use crate::store::StoreError;

#[derive(Debug)]
pub enum OutcomeError {
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    MissingExecutionResult(VersionId),
    NotExecutionResult,
    MissingRegisteredSchema(SchemaId),
}

impl From<StoreError> for OutcomeError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for OutcomeError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for OutcomeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

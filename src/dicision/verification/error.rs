use crate::core::{ArtifactFactoryError, SchemaId, VersionId};
use crate::store::StoreError;

#[derive(Debug)]
pub enum VerificationError {
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    MissingTarget(VersionId),
    MissingRegisteredSchema(SchemaId),
}

impl From<StoreError> for VerificationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for VerificationError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for VerificationError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

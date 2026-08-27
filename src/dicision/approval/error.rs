use crate::core::{ArtifactFactoryError, SchemaId, VersionId};
use crate::store::StoreError;

#[derive(Debug)]
pub enum ApprovalError {
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    MissingTarget(VersionId),
    MissingVerificationReport(VersionId),
    NotVerificationReport,
    VerificationTargetMismatch,
    MissingVerificationRelation,
    CannotApproveFailedVerification,
    CannotApproveUnknownVerification,
    UnauthorizedApprover,
    MissingRegisteredSchema(SchemaId),
}

impl From<StoreError> for ApprovalError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for ApprovalError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for ApprovalError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

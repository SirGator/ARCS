use crate::adapters::{AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, VersionId};
use crate::store::StoreError;

#[derive(Debug)]
pub enum ActionError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    MissingApproval(VersionId),
    NotApprovalArtifact,
    ApprovalNotApproved,
    MissingApprovalRelation,
    MissingCandidate(VersionId),
    ApprovalTargetIsNotCandidate,
    CapabilityIsNotAct(CapabilityRef),
    ActCapabilityIsNotIdempotent(CapabilityRef),
    CandidateSchemaNotAccepted {
        capability: CapabilityRef,
        schema: SchemaId,
    },
    CapabilityNotRequired(CapabilityRef),
    MissingRegisteredSchema(SchemaId),
}

impl From<AdapterRegistryError> for ActionError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for ActionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for ActionError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for ActionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

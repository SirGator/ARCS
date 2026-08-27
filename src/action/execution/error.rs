use crate::adapters::{AdapterCallError, AdapterId, AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation, VersionId};
use crate::runtime::InvocationError;
use crate::store::StoreError;

#[derive(Debug)]
pub enum ExecutionError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    AdapterCall(AdapterCallError),
    Invocation(InvocationError),
    MissingAction(VersionId),
    NotActionArtifact,
    UntrustedActionArtifact,
    MissingApproval(VersionId),
    NotApprovalArtifact,
    ApprovalNotApproved,
    UnauthorizedApprovalActor,
    MissingCandidate(VersionId),
    ApprovalTargetIsNotCandidate,
    ApprovalTargetMismatch,
    ActionInputSchemaMismatch,
    ActionPayloadMismatch,
    MissingActionTargetRelation,
    MissingActionApprovalRelation,
    MissingApprovalRelation,
    MissingVerificationBasisRelation,
    EndpointAdapterMismatch,
    CapabilityIsNotAct(CapabilityRef),
    ActCapabilityIsNotIdempotent(CapabilityRef),
    CapabilityNotRequired(CapabilityRef),
    ExecutorClassRequired(AdapterId),
    InputSchemaNotAccepted {
        capability: CapabilityRef,
        schema: SchemaId,
    },
    InvalidResultSchemaCount {
        actual: usize,
    },
    MissingRegisteredSchema(SchemaId),
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

impl From<AdapterRegistryError> for ExecutionError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for ExecutionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ArtifactFactoryError> for ExecutionError {
    fn from(value: ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for ExecutionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

impl From<AdapterCallError> for ExecutionError {
    fn from(value: AdapterCallError) -> Self {
        Self::AdapterCall(value)
    }
}

impl From<InvocationError> for ExecutionError {
    fn from(value: InvocationError) -> Self {
        Self::Invocation(value)
    }
}

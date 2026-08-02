use crate::adapters::{
    AdapterCallError, AdapterId, AdapterRegistryError, CapabilityId, CapabilityRef,
};
use crate::core::{SchemaId, SchemaViolation, VersionId};
use crate::store::StoreError;

/// Fehler ausschließlich innerhalb des kuratierten Reasoning-Slices.
#[derive(Debug)]
pub enum ReasoningError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Serialization(serde_json::Error),
    AdapterCall(AdapterCallError),
    NotReasoningAdapter(AdapterId),
    ReasoningProducerMustBeModel(AdapterId),
    ReasoningBudgetExceedsGrant,
    UndeclaredOutputSchema {
        capability: CapabilityId,
        schema: SchemaId,
    },
    ReasoningOutputMustBeCandidate(SchemaId),
    MissingRegisteredSchema(SchemaId),
    UnknownAllowedCapability(CapabilityRef),
    MissingReasoningEndpoint(AdapterId),
    ReasoningRequestAlreadyUsed {
        capability: CapabilityRef,
        request_id: String,
    },
    InvalidReasoningRequest(String),
    MissingContextArtifact(VersionId),
    DuplicateContextArtifact(VersionId),
    InvalidContextField {
        version: VersionId,
        field: String,
    },
    ContextPayloadMustBeObject(VersionId),
    ContextTooLarge {
        actual: usize,
        maximum: usize,
    },
    ResponseRequestMismatch,
    ResponseTooLarge {
        actual: usize,
        maximum: usize,
    },
    TooManyCandidates {
        actual: usize,
        maximum: usize,
    },
    InvalidReasoningTrace,
    UnexpectedCandidateSchema(SchemaId),
    InvalidPayload(Vec<SchemaViolation>),
    ForbiddenCandidateCapability(CapabilityRef),
    CandidateReferenceOutsideContext(VersionId),
    ProposalAlreadyCommitted {
        adapter: AdapterId,
        request_id: String,
        candidate_index: usize,
    },
}

impl From<AdapterRegistryError> for ReasoningError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for ReasoningError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<serde_json::Error> for ReasoningError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

impl From<AdapterCallError> for ReasoningError {
    fn from(value: AdapterCallError) -> Self {
        Self::AdapterCall(value)
    }
}

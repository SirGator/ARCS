use crate::adapters::{AdapterCallError, AdapterId, AdapterRegistryError, CapabilityRef};
use crate::core::{ArtifactFactoryError, SchemaId, SchemaViolation, VersionId};
use crate::runtime::InvocationError;
use crate::runtime::effects::EffectError;
use crate::runtime::reservations::ReservationError;
use crate::store::StoreError;

#[derive(Debug)]
pub enum ExecutionError {
    Authorization(AdapterRegistryError),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
    AdapterCall(AdapterCallError),
    Invocation(InvocationError),
    RecordedFailure {
        invocation_id: String,
        detail: String,
    },
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
    ReservationFenceMismatch(CapabilityRef),
    Reservation(ReservationError),
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
    /// Das Ergebnis einer externen Wirkung ist ungeklärt. Die Wirkung wird
    /// nicht wiederholt, bis sie ausdrücklich geklärt wurde.
    UnclearExternalEffect {
        operation_id: String,
        detail: String,
    },
    /// Das Buch der externen Wirkungen hat die Änderung nicht angenommen.
    Effect(EffectError),
    /// Die externe Wirkung gehört einer anderen Aufgabe. Ihre Ergebnisse
    /// dürfen von dieser Aufgabe nicht übernommen werden.
    EffectBindingMismatch {
        operation_id: String,
        expected: String,
        actual: String,
    },
    /// Der Auftrag verlässt die freigegebene Grenze der Installation.
    Boundary(crate::privacy::BoundaryViolation),
    /// Eine Geheimnisfreigabe hat den Einsatz nicht getragen.
    Secret(crate::privacy::SecretError),
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

impl From<EffectError> for ExecutionError {
    fn from(value: EffectError) -> Self {
        Self::Effect(value)
    }
}

impl From<crate::privacy::BoundaryViolation> for ExecutionError {
    fn from(value: crate::privacy::BoundaryViolation) -> Self {
        Self::Boundary(value)
    }
}

impl From<crate::privacy::SecretError> for ExecutionError {
    fn from(value: crate::privacy::SecretError) -> Self {
        Self::Secret(value)
    }
}

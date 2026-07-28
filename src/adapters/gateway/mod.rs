//! Stabile Gateway-Fassade über die vertikalen Adapter-Use-Cases.
//!
//! Dieses Modul hält nur gemeinsamen Zustand und Fehler. Registrierung,
//! Observation-Ingest, Reasoning und Proposal-Commit liegen in getrennten
//! Slices und erweitern `AdapterGateway` über eigene `impl`-Blöcke.

use std::collections::{HashMap, HashSet};

use crate::adapters::data::DataAdapter;
use crate::adapters::output::OutputAdapter;
use crate::adapters::port::AdapterCallError;
use crate::adapters::reasoning::ReasoningAdapter;
use crate::adapters::registration::{
    AdapterId, AdapterRegistry, AdapterRegistryError, CapabilityId, CapabilityRef,
};
use crate::core::{
    ArtifactIdGenerator, Clock, RegistryError, SchemaId, SchemaRegistry, SchemaViolation, VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError};

mod connection;
mod data;
mod envelope;
mod internal;
mod output;
mod proposal;
mod reasoning;
mod registration;
mod support;

pub use connection::AdapterConnectionError;
pub use internal::InternalArtifactSubmission;
pub use support::*;

/// Fehler an der einzigen mutierenden Grenze für externe Adapterdaten.
#[derive(Debug)]
pub enum AdapterGatewayError {
    AdapterRegistry(AdapterRegistryError),
    SchemaRegistry(RegistryError),
    Store(StoreError),
    Serialization(serde_json::Error),
    DuplicateReasoningEndpoint(AdapterId),
    DuplicateDataEndpoint(AdapterId),
    DuplicateOutputEndpoint(AdapterId),
    CapabilityRequiresDedicatedEndpoint(CapabilityRef),
    NotReasoningAdapter(AdapterId),
    NotDataAdapter(AdapterId),
    NotOutputAdapter(AdapterId),
    ReasoningProducerMustBeModel(AdapterId),
    DataProducerMustNotBeModel(AdapterId),
    OutputProducerMustBeExecutor(AdapterId),
    SessionTokenExhausted,
    UndeclaredOutputSchema {
        capability: CapabilityId,
        schema: SchemaId,
    },
    ReasoningOutputMustBeCandidate(SchemaId),
    MissingRegisteredSchema(SchemaId),
    InvalidBoundaryReference,
    ExternalReferenceTooLarge {
        actual: usize,
        maximum: usize,
    },
    PayloadTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidPayload(Vec<SchemaViolation>),
    InvalidReasoningRequest(String),
    ReasoningRequestAlreadyUsed {
        capability: CapabilityRef,
        request_id: String,
    },
    ReasoningBudgetExceedsGrant,
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
    UnknownAllowedCapability(CapabilityRef),
    MissingReasoningEndpoint(AdapterId),
    MissingDataEndpoint(AdapterId),
    MissingOutputEndpoint(AdapterId),
    MissingInputArtifact(VersionId),
    MissingRequestSubject(VersionId),
    InputSchemaNotAccepted {
        capability: CapabilityRef,
        schema: SchemaId,
    },
    InvocationResponseMismatch,
    InvalidInternalSubmission,
    InvocationAlreadyCompleted {
        capability: CapabilityRef,
        input: VersionId,
        response_schema: SchemaId,
    },
    AdapterCall(AdapterCallError),
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
    ForbiddenCandidateCapability(CapabilityRef),
    CandidateReferenceOutsideContext(VersionId),
    ProposalAlreadyCommitted {
        adapter: AdapterId,
        request_id: String,
        candidate_index: usize,
    },
}

impl From<AdapterRegistryError> for AdapterGatewayError {
    fn from(value: AdapterRegistryError) -> Self {
        Self::AdapterRegistry(value)
    }
}

impl From<RegistryError> for AdapterGatewayError {
    fn from(value: RegistryError) -> Self {
        Self::SchemaRegistry(value)
    }
}

impl From<StoreError> for AdapterGatewayError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<serde_json::Error> for AdapterGatewayError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

impl From<AdapterCallError> for AdapterGatewayError {
    fn from(value: AdapterCallError) -> Self {
        Self::AdapterCall(value)
    }
}

/// Kontrollierte Grenze zwischen ARCS und extern installierten Adaptern.
///
/// Adapter erhalten nie Referenzen auf Store, Network oder andere Adapter.
/// Der Gateway prüft Session, Capability, Betreiber-Grant und Schema, bevor
/// irgendeine externe Payload Teil des Cores werden kann.
pub struct AdapterGateway<'a> {
    instance_id: GatewayInstanceId,
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
    registry: AdapterRegistry,
    reasoning_endpoints: HashMap<AdapterId, Box<dyn ReasoningAdapter>>,
    data_endpoints: HashMap<AdapterId, Box<dyn DataAdapter>>,
    output_endpoints: HashMap<AdapterId, Box<dyn OutputAdapter>>,
    adapter_sessions: HashMap<u64, AdapterId>,
    next_session_token: u64,
    committed_proposals: HashSet<(VersionId, usize)>,
    used_reasoning_requests: HashSet<(CapabilityRef, String)>,
    completed_data_invocations: HashSet<(CapabilityRef, VersionId, SchemaId)>,
    completed_output_invocations: HashSet<(CapabilityRef, VersionId, SchemaId)>,
    clock: Box<dyn Clock>,
    ids: Box<dyn ArtifactIdGenerator>,
}

impl<'a> AdapterGateway<'a> {
    pub fn new(
        schemas: &'a mut SchemaRegistry,
        store: &'a SqliteArtifactStore,
        clock: Box<dyn Clock>,
        ids: Box<dyn ArtifactIdGenerator>,
    ) -> Self {
        // Bei einer theoretischen Erschöpfung des prozessweiten ID-Raums darf
        // kein Gateway mit einer wiederverwendeten Identität entstehen.
        let instance_id = GatewayInstanceId::allocate()
            .expect("gateway instance identity space exhausted; refusing unsafe reuse");

        Self {
            instance_id,
            schemas,
            store,
            registry: AdapterRegistry::new(),
            reasoning_endpoints: HashMap::new(),
            data_endpoints: HashMap::new(),
            output_endpoints: HashMap::new(),
            adapter_sessions: HashMap::new(),
            next_session_token: 1,
            committed_proposals: HashSet::new(),
            used_reasoning_requests: HashSet::new(),
            completed_data_invocations: HashSet::new(),
            completed_output_invocations: HashSet::new(),
            clock,
            ids,
        }
    }

    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    pub(crate) fn store(&self) -> &'a SqliteArtifactStore {
        self.store
    }

    /// Read-only-Sicht auf die vom Gateway verwendeten Verträge.
    ///
    /// Runtime-Policies dürfen damit ihre Konfiguration prüfen, erhalten aber
    /// keine Möglichkeit, Adapterdaten an der Gateway-Grenze vorbei einzufügen.
    pub(crate) fn schemas(&self) -> &SchemaRegistry {
        self.schemas
    }
}

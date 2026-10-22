use crate::adapters::{AdapterCallError, AdapterId, AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, VersionId};
pub use arcs_adapter_protocol::reasoning::*;
use serde_json::Value;

/// Interner Port zu einem externen LLM, Planner oder Solver.
///
/// Ein reales Plugin implementiert denselben seriellen DTO-Vertrag später über
/// HTTP, stdio oder IPC. Das Trait selbst ist keine stabile Plugin-ABI und
/// verleiht keinerlei Store-, Network- oder Execution-Zugriff.
pub trait ReasoningAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn propose(&self, request: &ReasoningInvocation)
    -> Result<ReasoningResponse, AdapterCallError>;
}

/// Vollständig geprüfter, weiterhin nicht autorisierter Modellvorschlag.
///
/// Öffentliche Konstruktion ist absichtlich nicht möglich. Nur der Gateway
/// kann aus einer untrusted `ProposalSubmission` diesen Zustand erzeugen.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedProposal {
    pub(crate) adapter_id: AdapterId,
    pub(crate) reasoning_capability: CapabilityRef,
    pub(crate) request_id: String,
    /// Exakte Core-Version des Audit-Artefakts, das vor dem externen
    /// Reasoning-Aufruf gespeichert wurde.
    pub(crate) reasoning_request_version: VersionId,
    pub(crate) candidate_index: usize,
    pub(crate) schema_id: SchemaId,
    pub(crate) required_capabilities: Vec<CapabilityRef>,
    pub(crate) referenced_versions: Vec<VersionId>,
    pub(crate) context_versions: Vec<VersionId>,
    pub(crate) payload: Value,
    pub(crate) trace: ReasoningTrace,
}

impl ValidatedProposal {
    pub fn adapter_id(&self) -> &AdapterId {
        &self.adapter_id
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn reasoning_request_version(&self) -> &VersionId {
        &self.reasoning_request_version
    }

    pub fn schema_id(&self) -> &SchemaId {
        &self.schema_id
    }

    pub fn required_capabilities(&self) -> &[CapabilityRef] {
        &self.required_capabilities
    }

    pub fn referenced_versions(&self) -> &[VersionId] {
        &self.referenced_versions
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }
}

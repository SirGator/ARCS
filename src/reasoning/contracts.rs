use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::port::AdapterCallError;
use crate::adapters::registration::{AdapterId, AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, TrustLevel, VersionId};

/// Betreiberseitige Kosten- und Größenobergrenzen eines ReasoningAdapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningLimits {
    pub max_context_items: usize,
    pub max_context_bytes: usize,
    pub max_output_tokens: usize,
    pub max_output_bytes: usize,
    pub max_candidates: usize,
}

impl ReasoningLimits {
    pub(crate) fn all_positive(&self) -> bool {
        self.max_context_items > 0
            && self.max_context_bytes > 0
            && self.max_output_tokens > 0
            && self.max_output_bytes > 0
            && self.max_candidates > 0
    }
}

/// Explizite Whitelist eines bereits gespeicherten Kontextartefakts.
///
/// Nur die genannten Top-Level-Payloadfelder verlassen die Core-Grenze.
/// Envelope-Metadaten, Source-Referenzen, Tags und der übrige Store bleiben
/// grundsätzlich verborgen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSelection {
    pub version_id: VersionId,
    pub payload_fields: Vec<String>,
}

/// Harte Kosten- und Größenbegrenzung eines Reasoning-Aufrufs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningBudget {
    pub max_context_items: usize,
    pub max_context_bytes: usize,
    pub max_output_tokens: usize,
    pub max_output_bytes: usize,
    pub max_candidates: usize,
}

impl ReasoningBudget {
    /// Ein Request darf die Betreibergrenzen nur verkleinern.
    pub(crate) fn fits_within(&self, limits: &ReasoningLimits) -> bool {
        self.max_context_items <= limits.max_context_items
            && self.max_context_bytes <= limits.max_context_bytes
            && self.max_output_tokens <= limits.max_output_tokens
            && self.max_output_bytes <= limits.max_output_bytes
            && self.max_candidates <= limits.max_candidates
    }
}

/// Core-seitiger Auftrag, bevor Kontextdaten aufgelöst werden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningRequest {
    pub request_id: String,
    /// Konkreter Reasoning-Port; nackte Capability-Namen sind absichtlich
    /// nicht ausreichend.
    pub reasoning_capability: CapabilityRef,
    pub objective: String,
    pub context: Vec<ContextSelection>,
    pub target_schema_id: SchemaId,
    /// Fähigkeiten, über die das Modell nachdenken darf. Diese IDs sind reine
    /// Information und niemals ausführbare Berechtigungstokens.
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub constraints: Value,
    pub budget: ReasoningBudget,
}

/// Minimierte Sicht auf ein einzelnes Kontextartefakt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningContextItem {
    pub version_id: VersionId,
    pub schema_id: SchemaId,
    pub artifact_type: String,
    pub trust_level: TrustLevel,
    pub payload: Value,
}

/// Tatsächlich an den externen ReasoningAdapter übergebener Wire-Vertrag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningInvocation {
    /// Stabile, vom Core erzeugte Invocation-ID. Der externe Modellservice
    /// muss sie ebenfalls deduplizieren.
    pub invocation_id: String,
    pub request_id: String,
    /// Exakt die autorisierte Reasoning-Fähigkeit, die diesen Auftrag erhält.
    ///
    /// Das ist besonders für Adapter mit mehreren Modell-, Planner- oder
    /// Solver-Capabilities nötig; eine Adapter-ID allein bestimmt keinen Port.
    pub capability: CapabilityRef,
    pub objective: String,
    pub context: Vec<ReasoningContextItem>,
    pub target_schema_id: SchemaId,
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub constraints: Value,
    pub max_output_tokens: usize,
    pub max_candidates: usize,
}

/// Untrusted Vorschlag eines ReasoningAdapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalSubmission {
    pub schema_id: SchemaId,
    pub required_capabilities: Vec<CapabilityRef>,
    pub referenced_versions: Vec<VersionId>,
    pub payload: Value,
}

/// Vom Adapter berichtete, nicht autorisierende Modellspur.
///
/// Die Hashwerte werden in diesem Slice noch nicht kryptographisch durch den
/// Core nachgerechnet und dürfen deshalb nicht als Vertrauensbeweis gelten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningTrace {
    pub model_name: String,
    pub prompt_hash: String,
    pub raw_output_hash: String,
    pub temperature: f64,
}

/// Antwort des externen Reasoning-Ports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningResponse {
    pub invocation_id: String,
    pub request_id: String,
    pub candidates: Vec<ProposalSubmission>,
    pub trace: ReasoningTrace,
}

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

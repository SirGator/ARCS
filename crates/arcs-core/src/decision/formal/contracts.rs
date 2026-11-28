//! Fachliche Verträge für formale Aussagen, Nachweise und Nachweisbedarf.

use serde::{Deserialize, Serialize};

use arcs_adapter_protocol::formal::{
    FormalCheckOutcome, FormalCheckRequest, FormalFinding, FormalVerdict, Toolchain,
};

use crate::core::VersionId;

/// Herkunft einer formalen Annahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssumptionOrigin {
    /// Ausdrücklich vom Betreiber oder Nutzer gesetzt.
    User,
    /// Aus anderen Artefakten abgeleitet.
    Derived,
    /// Aus einem externen Formalismus übernommen.
    Imported,
    /// Ausdrücklich als Axiom zugelassen.
    Axiom,
}

impl AssumptionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Derived => "derived",
            Self::Imported => "imported",
            Self::Axiom => "axiom",
        }
    }
}

/// Formale Definition, die eine Aussage benötigt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionDraft {
    pub name: String,
    pub body: String,
    pub source_versions: Vec<VersionId>,
}

/// Formale Annahme im Beweisraum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssumptionDraft {
    pub name: String,
    pub statement: String,
    pub origin: AssumptionOrigin,
    pub source_versions: Vec<VersionId>,
}

/// Abstraktes Modell eines Systems oder Prozesses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDraft {
    pub name: String,
    pub body: String,
    pub abstraction: Option<String>,
    pub source_versions: Vec<VersionId>,
}

/// Eine zu beweisende Aussage.
///
/// Die Obligation benennt Sollzustände, Vor- und Nachbedingungen, Invarianten
/// und Ressourcenlimits. Sie ist damit die Stelle, an der eine Anforderung
/// formal ausgedrückt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObligationDraft {
    pub name: String,
    pub statement: String,
    pub assumption_names: Vec<String>,
    pub definition_names: Vec<String>,
    /// Handlung, für die die Aussage gelten soll.
    pub target_version: Option<VersionId>,
    pub model_version: Option<VersionId>,
    pub plan_revision: Option<String>,
    pub goal: Option<String>,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
    pub invariants: Vec<String>,
    pub resource_limits: Option<ResourceLimits>,
    /// Axiome, die ein Nachweis zu dieser Obligation verwenden darf.
    pub allowed_axioms: Vec<String>,
    pub source_versions: Vec<VersionId>,
}

/// Ressourcenlimits, die eine Obligation formal begrenzt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_model_calls: u64,
    pub max_tokens: u64,
    pub max_cost_micros: u64,
    pub max_wall_clock_ms: u64,
}

/// Ein Beweisvorschlag zu einer Obligation.
///
/// Der Vorschlag ist noch kein Nachweis. Er wird erst nach einer unabhängigen
/// Prüfung durch ein formales Werkzeug zu Evidenz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofDraft {
    pub name: String,
    pub statement: String,
    pub assumption_names: Vec<String>,
    pub definition_names: Vec<String>,
    pub obligation_version: Option<VersionId>,
    /// Handlung, die der Beweisvorschlag belegen soll.
    pub target_version: Option<VersionId>,
    pub model_version: Option<VersionId>,
    pub plan_revision: Option<String>,
    pub toolchain: Toolchain,
    pub allowed_axioms: Vec<String>,
    pub proof_text: String,
    pub prover: Option<String>,
    pub source_versions: Vec<VersionId>,
}

/// Eine bewiesene Aussage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheoremDraft {
    pub name: String,
    pub statement: String,
    pub proof_version: VersionId,
    pub assumption_names: Vec<String>,
    pub model_version: Option<VersionId>,
    pub source_versions: Vec<VersionId>,
}

/// Ein Gegenbeispiel zu einer Aussage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterexampleDraft {
    pub name: String,
    pub statement: String,
    pub witness: String,
    pub toolchain: Toolchain,
    pub obligation_version: Option<VersionId>,
    pub source_versions: Vec<VersionId>,
}

/// Der Bezug, für den ein Nachweis ausgewertet wird.
///
/// Eine Obligation benennt selbst, an welchen Kontext sie gebunden ist. Der
/// Aufrufer liefert hier den tatsächlichen Kontext der Handlung. Beide Seiten
/// werden Feld für Feld verglichen: Was die Obligation nicht erklärt, kann
/// der Nachweis nicht einschließen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvidenceBinding {
    /// Konkrete Handlung, für die der Nachweis gelten soll.
    pub target: Option<VersionId>,
    /// Planrevision, an die der Nachweis gebunden sein muss.
    pub plan_revision: Option<String>,
    /// Ziel, an das der Nachweis gebunden sein muss.
    pub goal: Option<String>,
}

impl EvidenceBinding {
    /// Bindet den Nachweis an eine konkrete Handlung.
    pub fn target(target: VersionId) -> Self {
        Self {
            target: Some(target),
            ..Default::default()
        }
    }

    /// Bindet den Nachweis an eine Planrevision und ein Ziel.
    pub fn plan(plan_revision: Option<String>, goal: Option<String>) -> Self {
        Self {
            plan_revision,
            goal,
            ..Default::default()
        }
    }

    /// Vergleicht den geforderten Bezug mit dem Bezug einer Obligation.
    ///
    /// Jedes Feld, das die Obligation erklärt, muss übereinstimmen. Was die
    /// Obligation nicht erklärt, wird nicht erzwungen; ein fehlendes Feld ist
    /// kein freier Pass für einen beliebigen Kontext.
    pub fn mismatch(
        &self,
        target_version: Option<&str>,
        plan_revision: Option<&str>,
        goal: Option<&str>,
    ) -> Option<&'static str> {
        if let Some(declared) = target_version {
            match &self.target {
                Some(target) if target.0 == declared => {}
                _ => return Some("the obligation does not belong to the requested action"),
            }
        }
        if let Some(declared) = plan_revision {
            match &self.plan_revision {
                Some(revision) if revision == declared => {}
                _ => return Some("the obligation belongs to a different plan revision"),
            }
        }
        if let Some(declared) = goal {
            match &self.goal {
                Some(goal) if goal == declared => {}
                _ => return Some("the obligation belongs to a different goal"),
            }
        }
        None
    }
}

/// Ergebnis einer Nachweisprüfung an einer konkreten Aktion.
///
/// Die Stufen unterscheiden sich danach, ob ein Wartezustand sinnvoll ist:
/// Ein fehlgeschlagener Nachweis ist endgültig, ein nicht verfügbares
/// Werkzeug dagegen vorübergehend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceDecision {
    /// Der Nachweis ist gültig; die Aktion darf wirken.
    Allowed,
    /// Der Nachweis fehlt oder ist ungültig; die Aktion wird blockiert.
    Blocked(String),
    /// Der Nachweis ist noch nicht verfügbar; die Aktion wartet.
    Waiting(String),
}

impl EvidenceDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allowed => None,
            Self::Blocked(reason) | Self::Waiting(reason) => Some(reason),
        }
    }
}

/// Fachlicher Prüfport für ein externes formales Werkzeug.
///
/// Der Port kennt weder Store noch Runtime-Autorität. Das Werkzeug darf nur
/// prüfen; über die Verwendbarkeit des Ergebnisses entscheidet der Core.
pub trait FormalChecker {
    fn check(&self, request: &FormalCheckRequest) -> Result<FormalCheckOutcome, FormalError>;
}

/// Eine aus einer Obligation abgeleitete Prüfanfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObligationCheck {
    pub obligation_version: VersionId,
    pub statement: String,
    pub assumption_names: Vec<String>,
    pub model_version: Option<VersionId>,
    pub plan_revision: Option<String>,
    pub toolchain: Toolchain,
    pub allowed_axioms: Vec<String>,
    pub max_runtime_ms: u64,
}

/// Das Ergebnis einer formalen Prüfung inklusive Herkunftsnachweis.
///
/// Der Bericht nennt die geprüfte Aussage, das Werkzeug, die verwendeten
/// Axiome und das Urteil. Er ist die einzige Stelle, an der eine aus
/// unabhängiger Prüfung stammende Aussage als Nachweis verwendet wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormalReport {
    /// Handlung, für die der Nachweis gilt, falls er an eine gebunden ist.
    pub target_version: Option<String>,
    pub verdict: FormalVerdict,
    pub statement: String,
    pub used_axioms: Vec<String>,
    pub allowed_axioms: Vec<String>,
    pub findings: Vec<FormalFinding>,
    pub duration_ms: u64,
    pub obligation_version: Option<String>,
    pub model_version: Option<String>,
    pub plan_revision: Option<String>,
    /// Geprüfter Beweisvorschlag, falls das Werkzeug einen Beweis vorgelegt hat.
    pub proof_version: Option<String>,
    /// Prüfanfrage, aus der der Bericht hervorgegangen ist.
    pub request_version: Option<String>,
    /// Die Version des Berichts selbst im Core-Artefaktnetz.
    pub report_version: Option<String>,
}

/// Fehler des formalen Nachweises.
#[derive(Debug)]
pub enum FormalError {
    Store(crate::store::StoreError),
    Factory(crate::core::ArtifactFactoryError),
    Serialization(serde_json::Error),
    /// Der referenzierte Nachweis oder die Obligation fehlt.
    MissingVersion(VersionId),
    /// Das Artefakt hat nicht den erwarteten formalen Vertrag.
    WrongContract(String),
    /// Eine erforderliche Angabe fehlt oder ist ungültig.
    Invalid(String),
    /// Das formale Werkzeug hat einen Fehler gemeldet.
    Tool(String),
}

impl From<crate::store::StoreError> for FormalError {
    fn from(value: crate::store::StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::core::ArtifactFactoryError> for FormalError {
    fn from(value: crate::core::ArtifactFactoryError) -> Self {
        Self::Factory(value)
    }
}

impl From<serde_json::Error> for FormalError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

impl std::fmt::Display for FormalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "formal store: {error:?}"),
            Self::Factory(error) => write!(f, "formal artifact: {error:?}"),
            Self::Serialization(error) => write!(f, "formal payload: {error}"),
            Self::MissingVersion(version) => write!(f, "formal version missing: {version:?}"),
            Self::WrongContract(detail) => write!(f, "formal contract: {detail}"),
            Self::Invalid(detail) => write!(f, "formal invalid: {detail}"),
            Self::Tool(detail) => write!(f, "formal tool: {detail}"),
        }
    }
}

impl std::error::Error for FormalError {}

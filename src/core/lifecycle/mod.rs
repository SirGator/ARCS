//! Append-only Beschreibung beobachtbarer Systemereignisse.
//!
//! Ein Ereignis ersetzt keine Artefaktversion. Es dokumentiert, was mit
//! welchen Versionen geschehen ist, und ermöglicht Audit und Replay.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::artifact::{Actor, ArtifactId, VersionId};

/// Kontrollierte Ereignisklassen des ARCS-Kerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Eine gültige Artefaktversion wurde dauerhaft gespeichert.
    ArtifactCommitted,
    /// Der sichtbare Kopf eines Streams wurde weitergesetzt.
    HeadAdvanced,
    /// Eine Freigabe oder Ablehnung wurde aufgezeichnet.
    ApprovalRecorded,
    /// Aus einem geprüften Kandidaten entstand eine ausführbare Aktion.
    ActionMaterialized,
    /// Das tatsächliche Ergebnis einer Ausführung wurde gespeichert.
    ExecutionRecorded,
    /// Der Core hat eine kontrollierte Entscheidung festgehalten.
    DecisionRecorded,
}

/// Semantische Rolle einer Artefaktverknüpfung in einem Ereignis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceRole {
    /// Hauptgegenstand des Ereignisses.
    Target,
    /// Direkter Vorgänger oder Auslöser.
    Parent,
    /// Policy-Version, gegen die geprüft wurde.
    Policy,
    /// Betroffene oder erzeugte Aktion.
    Action,
    /// Ergebnis einer Ausführung oder Prüfung.
    Result,
}

/// Unveränderlicher Verweis auf eine bestimmte Artefaktversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Logische Artefaktidentität.
    pub artifact_id: ArtifactId,
    /// Exakte, für die Entscheidung verwendete Version.
    pub version_id: VersionId,
    /// Bedeutung der Referenz in diesem Ereignis.
    pub role: ReferenceRole,
}

/// Auditierbarer Eintrag im Lifecycle- beziehungsweise Event-Log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleEvent {
    /// Global eindeutige Ereignisidentität.
    pub event_id: String,
    /// Fachliche Art des Ereignisses.
    pub event_type: EventType,
    /// Erstellungszeitpunkt im RFC-3339-Format.
    pub ts: String,
    /// Komponente oder Person, die das Ereignis ausgelöst hat.
    pub actor: Actor,
    /// Artefaktversionen, auf die sich das Ereignis bezieht.
    pub refs: Vec<ArtifactRef>,
    /// Zugehöriger Ablauf- oder Kontextstream.
    pub stream_key: String,
    /// Ereignisspezifische Zusatzdaten.
    #[serde(default)]
    pub payload: Value,
    /// Optionaler Vorgänger-Hash für eine manipulationssichtbare Kette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
}

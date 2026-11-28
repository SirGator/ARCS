//! Gemeinsamer Vertrag für externe formale Prüfwerkzeuge.
//!
//! Das Protokoll beschreibt nur, was ein Werkzeug meldet. Es erteilt keine
//! Berechtigung und behauptet keine Verifikation: Ob ein Ergebnis als
//! Nachweis gilt, entscheidet der Core anhand von Aussage, Annahmen,
//! Modellversion, Werkzeugkette, erlaubten Axiomen und Prüfergebnis.

use serde::{Deserialize, Serialize};

/// Version des formalen Prüfvertrags.
pub const FORMAL_PROTOCOL_VERSION: u32 = 1;

/// Versionsangabe der verwendeten Werkzeugkette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    /// Name des Werkzeugs, zum Beispiel `lean4`.
    pub name: String,
    /// Konkrete Version des Werkzeugs.
    pub version: String,
}

/// Ergebnis eines einzelnen Prüfschritts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormalFindingVerdict {
    Pass,
    Fail,
    Unknown,
}

/// Ein einzelner Befund einer formalen Prüfung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalFinding {
    pub check: String,
    pub verdict: FormalFindingVerdict,
    pub detail: String,
}

/// Gesamtergebnis einer formalen Prüfung.
///
/// Die Verdict-Stufen trennen ausdrücklich „nicht bewiesen“ von „bewiesen“.
/// Nur `Proved` darf nach einer Zulässigkeitsprüfung als Nachweis gelten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormalVerdict {
    /// Die Aussage wurde bewiesen.
    Proved,
    /// Die Aussage wurde widerlegt.
    Disproved,
    /// Die Prüfung lief, der Nachweis ist aber unbrauchbar.
    Failed,
    /// Die Prüfung konnte nicht ausgeführt werden.
    Unavailable,
    /// Der Nachweis verstößt gegen die Zulässigkeitsregeln.
    Rejected,
}

impl FormalVerdict {
    /// Nur `Proved` ist ein gültiger Nachweis.
    pub fn is_proof(self) -> bool {
        matches!(self, Self::Proved)
    }
}

/// Ein Definitionsteil, den die Aussage benötigt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalSource {
    pub name: String,
    pub body: String,
}

/// Eine Annahme, die im Beweisraum als Axiom gilt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalAssumption {
    pub name: String,
    pub statement: String,
}

/// Anfrage an ein externes formales Werkzeug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalCheckRequest {
    /// Zu beweisende Aussage in der Sprache des Werkzeugs.
    pub statement: String,
    /// Optionaler Beweisvorschlag. Ohne Vorschlag sucht das Werkzeug selbst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_text: Option<String>,
    /// Definitionen, die die Aussage benötigt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<FormalSource>,
    /// Annahmen, die als Axiome im Beweisraum gelten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<FormalAssumption>,
    /// Axiome, die der Nachweis verwenden darf.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_axioms: Vec<String>,
    /// Werkzeugkette, mit der geprüft wurde.
    pub toolchain: Toolchain,
    /// Harte Laufzeitgrenze der Prüfung.
    pub max_runtime_ms: u64,
}

/// Antwort eines externen formalen Werkzeugs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalCheckOutcome {
    pub verdict: FormalVerdict,
    /// Vom Werkzeug tatsächlich verwendete Axiome.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_axioms: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<FormalFinding>,
    /// Vom Werkzeug gefundener Beweis, falls es selbst einen suchen sollte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_text: Option<String>,
    pub duration_ms: u64,
}

/// Fehler eines externen formalen Werkzeugs.
///
/// Der Fehler ist kein Prüfergebnis: Ein Werkzeug, das nicht laufen konnte,
/// meldet `Unavailable` als Verdict, nicht einen Fehler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalToolError {
    pub code: String,
    pub message: String,
}

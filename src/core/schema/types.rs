use serde_json::Value;

use crate::core::artifact::SchemaId;

/// Ein registrierter JSON-Schema-Vertrag.
#[derive(Debug, Clone)]
pub struct SchemaDefinition {
    /// Stabile `$id` aus dem Schema-Dokument.
    pub id: SchemaId,
    /// Fachlicher ARCS-Artefakttyp, der aus der Schema-ID abgeleitet wurde.
    pub artifact_type: String,
    /// Numerische Vertragsversion aus dem Suffix der Schema-ID.
    pub version: u64,
    /// Geparstes JSON-Schema.
    pub document: Value,
}

/// Beschreibung einer einzelnen Vertragsverletzung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    /// JSON-Pfad der fehlerhaften oder fehlenden Eigenschaft.
    pub path: String,
    /// Menschenlesbare Fehlerbeschreibung.
    pub message: String,
}

impl SchemaViolation {
    /// Interner Konstruktor für einheitliche Fehlermeldungen.
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

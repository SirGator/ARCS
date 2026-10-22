use serde::{Deserialize, Serialize};

/// Eindeutige Identität genau einer unveränderlichen Artefaktversion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionId(pub String);

/// Verweist auf den Vertrag, nach dem der Payload validiert wird.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaId(pub String);

/// Fachliches Objekt, auf das sich ein Artifact bezieht.
///
/// Beispiele sind `server-01/cpu` oder `current_user_request`. Die konkrete
/// Semantik bleibt beim jeweiligen Adapter-Schema.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubjectId(pub String);

/// Grobe Vertrauensbewertung einer Quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Unbestätigte oder unzuverlässige Information.
    Low,
    /// Plausible, aber nicht vollständig bestätigte Information.
    Medium,
    /// Authentifizierte oder anderweitig stark bestätigte Information.
    High,
}

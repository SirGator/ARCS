//! Kontrollierter Kern von ARCS.
//!
//! Dieses Modul enthält keine LLM-Autorität und führt keine externen Aktionen
//! aus. Es definiert die gemeinsamen Datentypen und prüft, welche Artefakte
//! gültig sind.

/// Gemeinsamer, unveränderlicher Artefaktvertrag.
pub mod artifact;
/// Registry und Prüfung der versionierten JSON-Schemas.
pub mod schema;
/// Zusammengesetzte Validierung kompletter Artefakte.
pub mod validation;

pub use artifact::{
    Actor, ActorType, Artifact, ArtifactId, ModelUse, Provenance, SchemaId, Source, SourceClass,
    SourceKind, Trust, TrustLevel, VersionId,
};
pub use schema::{RegistryError, SchemaDefinition, SchemaRegistry, SchemaViolation};
pub use validation::{ValidationError, validate_artifact};

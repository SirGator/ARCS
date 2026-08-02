//! Kontrollierter Kern von ARCS.
//!
//! Dieses Modul enthält keine LLM-Autorität und führt keine externen Aktionen
//! aus. Es definiert die gemeinsamen Datentypen und prüft, welche Artefakte
//! gültig sind.

/// Gemeinsamer, unveränderlicher Artefaktvertrag.
pub mod artifact;
/// Zentrale Erzeugung von Artefaktidentitäten, Zeitstempeln und Umschlägen.
pub mod artifact_factory;
/// Registry und Prüfung der versionierten JSON-Schemas.
pub mod schema;
/// Zusammengesetzte Validierung kompletter Artefakte.
pub mod validation;

pub use artifact::{
    Actor, ActorType, Artifact, ArtifactId, MAX_ARTIFACT_TYPE_BYTES, MAX_MODEL_TRACE_TEXT_BYTES,
    MAX_SOURCE_REFERENCE_BYTES, ModelUse, Provenance, SchemaId, Source, SourceClass, SourceKind,
    SubjectId, Trust, TrustLevel, VersionId,
};
pub use artifact_factory::{
    ArtifactFactory, ArtifactFactoryError, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    GeneratedArtifactIds, SequenceIdGenerator, SystemClock,
};
pub use schema::{RegistryError, SchemaDefinition, SchemaRegistry, SchemaViolation};
pub use validation::{ValidationError, validate_artifact};

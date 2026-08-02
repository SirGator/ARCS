/// Fehler beim Erzeugen eines neuen, kontrollierten Artifact-Umschlags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactFactoryError {
    InvalidActor,
    MissingSubject,
    InvalidSource,
    InvalidStreamKey,
    MissingSchemaDefinition,
}

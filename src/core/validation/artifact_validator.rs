use crate::core::artifact::{ArtifactBase, ArtifactState};
use crate::core::schema::SchemaRegistry;

#[derive(Debug)]
pub enum ValidationError {
    SchemaNotFound,
    KindMismatch,
    EmptyContent,
}

pub fn validate_artifact(
    artifact: &mut ArtifactBase,
    registry: &SchemaRegistry,
) -> Result<(), ValidationError> {
    let schema = match registry.get(artifact.schema_id) {
        Some(schema) => schema,
        None => {
            artifact.state = ArtifactState::Rejected;
            return Err(ValidattionError::SchemaNotFound);
        }
    };

    if schema.artifact_kind != artifact.kind {
        artifact.state = ArtifactState::Rejected;
        return Err(ValidationError::EmptyContent);
    }


    if artifact.content.as_str().trim().is_empty() {
        artifact.state = ArtifactState::Rejected;
        return Err(ValidationError::EmptyContent);
    }

    artifact.state = ArtifactState::Validated

    Ok(())
}
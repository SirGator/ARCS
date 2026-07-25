use crate::core::artifact::Artifact;
use crate::core::schema::{SchemaRegistry, SchemaViolation};

/// Gründe, aus denen ein Artefakt nicht in den Store gelangen darf.
#[derive(Debug)]
pub enum ValidationError {
    /// Schema-ID, Artefakttyp, Schemaversion oder Payload sind ungültig.
    Payload(Vec<SchemaViolation>),
}

/// Validiert den ersten minimalen ARCS-Flow.
///
/// In diesem Slice ist die typisierte Rust-Struktur selbst der Vertrag für den
/// Artefakt-Umschlag. Die JSON-Schema-Prüfung gilt vorerst nur für den Payload.
/// Vor der Payload-Prüfung wird sichergestellt, dass Schema-ID, fachlicher Typ
/// und Schemaversion widerspruchsfrei zusammengehören.
pub fn validate_artifact(
    artifact: &Artifact,
    registry: &SchemaRegistry,
) -> Result<(), ValidationError> {
    let schema = registry.get(&artifact.schema_id).ok_or_else(|| {
        ValidationError::Payload(vec![SchemaViolation::new(
            "$.schema_id",
            "schema is not registered",
        )])
    })?;

    if artifact.artifact_type != schema.artifact_type {
        return Err(ValidationError::Payload(vec![SchemaViolation::new(
            "$.type",
            format!(
                "artifact type '{}' does not match schema type '{}'",
                artifact.artifact_type, schema.artifact_type
            ),
        )]));
    }

    if artifact.schema_version != schema.version {
        return Err(ValidationError::Payload(vec![SchemaViolation::new(
            "$.schema_version",
            format!(
                "schema version '{}' does not match registered version '{}'",
                artifact.schema_version, schema.version
            ),
        )]));
    }

    registry
        .validate(&artifact.schema_id, &artifact.payload)
        .map_err(ValidationError::Payload)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::core::artifact::{
        Actor, ActorType, Source, SourceClass, SourceKind, Trust, TrustLevel,
    };

    use super::*;

    fn input(payload: Value) -> Artifact {
        Artifact::new(
            "input-1",
            "input-1-v1",
            "input",
            "arcs.input.v1",
            "2026-07-26T23:00:00+02:00",
            Actor {
                actor_type: ActorType::Human,
                id: "simon".into(),
            },
            Source {
                kind: SourceKind::Chat,
                reference: "conversation-1".into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::Human,
            },
            "input:1",
            payload,
        )
    }

    #[test]
    // Ein Input mit nichtleerem Rohtext erfüllt den Minimalvertrag.
    fn valid_input_passes_payload_validation() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        assert!(validate_artifact(&input(json!({"raw_text": "Hallo ARCS"})), &registry).is_ok());
    }

    #[test]
    // Ein Input ohne Pflichtfeld darf den Core nicht passieren.
    fn malformed_input_fails_closed() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        assert!(matches!(
            validate_artifact(&input(json!({})), &registry),
            Err(ValidationError::Payload(_))
        ));
    }

    #[test]
    // Der fachliche Artefakttyp muss zur registrierten Schema-ID passen.
    fn mismatching_artifact_type_is_rejected() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let mut artifact = input(json!({"raw_text": "Hallo ARCS"}));
        artifact.artifact_type = "action".into();

        let Err(ValidationError::Payload(violations)) = validate_artifact(&artifact, &registry)
        else {
            panic!("mismatching artifact type must be rejected");
        };
        assert_eq!(violations[0].path, "$.type");
    }

    #[test]
    // Die numerische Version muss mit dem Suffix der Schema-ID übereinstimmen.
    fn mismatching_schema_version_is_rejected() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let mut artifact = input(json!({"raw_text": "Hallo ARCS"}));
        artifact.schema_version = 2;

        let Err(ValidationError::Payload(violations)) = validate_artifact(&artifact, &registry)
        else {
            panic!("mismatching schema version must be rejected");
        };
        assert_eq!(violations[0].path, "$.schema_version");
    }
}

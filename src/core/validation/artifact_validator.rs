use crate::core::artifact::{
    Artifact, MAX_ARTIFACT_TYPE_BYTES, MAX_MODEL_TRACE_TEXT_BYTES, MAX_SOURCE_REFERENCE_BYTES,
};
use crate::core::schema::{SchemaRegistry, SchemaViolation, is_rfc3339};

/// Gründe, aus denen ein Artefakt nicht in den Store gelangen darf.
#[derive(Debug)]
pub enum ValidationError {
    /// Schema-ID, Artefakttyp, Schemaversion oder Payload sind ungültig.
    Payload(Vec<SchemaViolation>),
}

/// Validiert den typisierten Artifact-Umschlag und seinen Payload-Vertrag.
///
/// Die Rust-Struktur verhindert falsche Grundtypen. Zusätzlich werden alle
/// sicherheits- und auditrelevanten Texte, Zeit, Version und Modelltemperaturen
/// fail-closed geprüft. JSON Schema kontrolliert anschließend den fachlichen
/// Payload; Schema-ID, Artifact-Typ und Schemaversion müssen zusammenpassen.
pub fn validate_artifact(
    artifact: &Artifact,
    registry: &SchemaRegistry,
) -> Result<(), ValidationError> {
    let mut envelope_violations = Vec::new();
    validate_envelope_text(
        "$.artifact_id",
        &artifact.artifact_id.0,
        512,
        &mut envelope_violations,
    );
    validate_envelope_text(
        "$.version_id",
        &artifact.version_id.0,
        512,
        &mut envelope_violations,
    );
    validate_envelope_text(
        "$.type",
        &artifact.artifact_type,
        MAX_ARTIFACT_TYPE_BYTES,
        &mut envelope_violations,
    );
    validate_envelope_text(
        "$.created_by.id",
        &artifact.created_by.id,
        512,
        &mut envelope_violations,
    );
    validate_envelope_text(
        "$.source.ref",
        &artifact.source.reference,
        MAX_SOURCE_REFERENCE_BYTES,
        &mut envelope_violations,
    );
    validate_envelope_text(
        "$.stream_key",
        &artifact.stream_key,
        1_024,
        &mut envelope_violations,
    );
    if artifact.version == 0 {
        envelope_violations.push(SchemaViolation::new("$.version", "must be at least 1"));
    }
    if !is_rfc3339(&artifact.created_at) {
        envelope_violations.push(SchemaViolation::new(
            "$.created_at",
            "must be a valid RFC 3339 date-time",
        ));
    }
    for (index, tag) in artifact.tags.iter().enumerate() {
        validate_envelope_text(
            &format!("$.tags[{index}]"),
            tag,
            1_024,
            &mut envelope_violations,
        );
    }
    if let Some(provenance) = &artifact.provenance {
        for (index, parent) in provenance.parents.iter().enumerate() {
            validate_envelope_text(
                &format!("$.provenance.parents[{index}]"),
                parent,
                512,
                &mut envelope_violations,
            );
        }
        for (index, model) in provenance.models_used.iter().enumerate() {
            validate_envelope_text(
                &format!("$.provenance.models_used[{index}].name"),
                &model.name,
                MAX_MODEL_TRACE_TEXT_BYTES,
                &mut envelope_violations,
            );
            validate_envelope_text(
                &format!("$.provenance.models_used[{index}].prompt_hash"),
                &model.prompt_hash,
                MAX_MODEL_TRACE_TEXT_BYTES,
                &mut envelope_violations,
            );
            validate_envelope_text(
                &format!("$.provenance.models_used[{index}].raw_output_hash"),
                &model.raw_output_hash,
                MAX_MODEL_TRACE_TEXT_BYTES,
                &mut envelope_violations,
            );
            if !model.temperature.is_finite() || model.temperature < 0.0 {
                envelope_violations.push(SchemaViolation::new(
                    format!("$.provenance.models_used[{index}].temperature"),
                    "must be finite and non-negative",
                ));
            }
        }
    }
    if !envelope_violations.is_empty() {
        return Err(ValidationError::Payload(envelope_violations));
    }

    if artifact.subject.as_ref().is_some_and(|subject| {
        subject.0.trim().is_empty()
            || subject.0.len() > 512
            || subject.0.chars().any(char::is_control)
    }) {
        return Err(ValidationError::Payload(vec![SchemaViolation::new(
            "$.subject",
            "subject must be non-empty, control-free, and at most 512 bytes",
        )]));
    }

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

fn validate_envelope_text(
    path: &str,
    value: &str,
    maximum_bytes: usize,
    violations: &mut Vec<SchemaViolation>,
) {
    if value.trim().is_empty() || value.len() > maximum_bytes || value.chars().any(char::is_control)
    {
        violations.push(SchemaViolation::new(
            path,
            format!("must be non-empty, control-free, and at most {maximum_bytes} bytes"),
        ));
    }
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

    #[test]
    fn invalid_envelope_metadata_is_rejected_before_payload_validation() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let mut artifact = input(json!({"raw_text": "Hallo ARCS"}));
        artifact.created_at = "not-a-date".into();
        artifact.created_by.id = " ".into();

        let Err(ValidationError::Payload(violations)) = validate_artifact(&artifact, &registry)
        else {
            panic!("invalid envelope metadata must be rejected");
        };
        assert!(
            violations
                .iter()
                .any(|violation| violation.path == "$.created_at")
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.path == "$.created_by.id")
        );
    }
}

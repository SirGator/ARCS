use crate::core::artifact::{Artifact, SchemaId};
use crate::core::schema::{SchemaRegistry, SchemaViolation};

/// Gründe, aus denen ein Artefakt nicht in den Store gelangen darf.
#[derive(Debug)]
pub enum ValidationError {
    /// Der typisierte Umschlag konnte nicht in JSON überführt werden.
    Serialization(serde_json::Error),
    /// Metadaten oder Struktur des gemeinsamen Umschlags sind ungültig.
    Envelope(Vec<SchemaViolation>),
    /// Der fachliche Payload verletzt seinen eigenen Vertrag.
    Payload(Vec<SchemaViolation>),
    /// `created_at` besitzt keine erkennbare RFC-3339-Struktur.
    InvalidTimestamp,
}

/// Validiert Umschlag und Payload eines Artefakts in zwei getrennten Stufen.
///
/// Erst wenn beide Verträge und der Zeitstempel gültig sind, darf das Artefakt
/// gespeichert werden. Ein fehlender Vertrag führt über die Registry ebenfalls
/// zu einem Fehler.
pub fn validate_artifact(
    artifact: &Artifact,
    registry: &SchemaRegistry,
) -> Result<(), ValidationError> {
    // Der gemeinsame Umschlag kontrolliert Identität, Herkunft, Vertrauen,
    // Versionierung und Provenienz unabhängig vom konkreten Artefakttyp.
    let envelope = serde_json::to_value(artifact).map_err(ValidationError::Serialization)?;
    registry
        .validate(&SchemaId("arcs.artifact_base.v1".into()), &envelope)
        .map_err(ValidationError::Envelope)?;

    // Danach wird ausschließlich der fachliche Inhalt gegen sein angegebenes
    // Schema geprüft.
    registry
        .validate(&artifact.schema_id, &artifact.payload)
        .map_err(ValidationError::Payload)?;

    // Das Schema deklariert `date-time`. Diese kleine zusätzliche Prüfung
    // verhindert beliebige Strings, ohne Zeitberechnungen in den Core zu holen.
    if !looks_like_rfc3339(&artifact.created_at) {
        return Err(ValidationError::InvalidTimestamp);
    }

    Ok(())
}

fn looks_like_rfc3339(value: &str) -> bool {
    // Geprüft wird bewusst nur die unverzichtbare Struktur. Semantische
    // Zeitrechnung gehört später in einen spezialisierten Zeittyp.
    let bytes = value.as_bytes();
    bytes.len() >= 20
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':')
        && (value.ends_with('Z')
            || bytes
                .get(19..)
                .is_some_and(|suffix| suffix.contains(&b'+') || suffix.contains(&b'-')))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::core::artifact::{
        Actor, ActorType, Source, SourceClass, SourceKind, Trust, TrustLevel,
    };

    use super::*;

    fn task(payload: Value) -> Artifact {
        // Gemeinsames Fixture; nur der Payload variiert je Test.
        Artifact::new(
            "artifact-1",
            "version-1",
            "task",
            "arcs.task.v1",
            "2026-07-25T18:00:00+02:00",
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
            "task:1",
            payload,
        )
    }

    #[test]
    // Belegt, dass Umschlag und Task-Payload gemeinsam akzeptiert werden.
    fn valid_task_passes_both_validation_layers() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        assert!(validate_artifact(&task(json!({"title": "Repair project"})), &registry).is_ok());
    }

    #[test]
    // Ein Modellvorschlag ohne Pflichtfelder darf den Core nicht passieren.
    fn malformed_task_fails_closed() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        assert!(matches!(
            validate_artifact(&task(json!({})), &registry),
            Err(ValidationError::Payload(_))
        ));
    }
}

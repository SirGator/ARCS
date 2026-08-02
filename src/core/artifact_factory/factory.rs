use serde_json::Value;

use crate::core::artifact::{Actor, Artifact, Provenance, Source, SubjectId, Trust};
use crate::core::schema::SchemaDefinition;

use super::{ArtifactFactoryError, ArtifactIdGenerator, Clock};

/// Bereits autorisierte und validierte Daten für ein neues Artifact.
///
/// Die Factory bestimmt ausschließlich interne Identitäten und Zeit. Der
/// aufrufende Use-Case bleibt für Autorisierung, Payload-Validierung und die
/// Auswahl der Schema-Definition verantwortlich.
pub struct ArtifactFactoryInput {
    pub schema: SchemaDefinition,
    pub created_by: Actor,
    pub source: Source,
    pub trust: Trust,
    pub stream_key: String,
    pub subject: SubjectId,
    pub tags: Vec<String>,
    pub payload: Value,
    pub provenance: Option<Provenance>,
}

/// Zentrale Erzeugung neuer, unveränderlicher Artifact-Umschläge.
pub struct ArtifactFactory<'a> {
    clock: &'a dyn Clock,
    ids: &'a mut dyn ArtifactIdGenerator,
}

impl<'a> ArtifactFactory<'a> {
    pub fn new(clock: &'a dyn Clock, ids: &'a mut dyn ArtifactIdGenerator) -> Self {
        Self { clock, ids }
    }

    pub fn create(
        &mut self,
        input: ArtifactFactoryInput,
    ) -> Result<Artifact, ArtifactFactoryError> {
        if input.schema.id.0.trim().is_empty()
            || input.schema.artifact_type.trim().is_empty()
            || input.schema.version == 0
        {
            return Err(ArtifactFactoryError::MissingSchemaDefinition);
        }
        if input.created_by.id.trim().is_empty() {
            return Err(ArtifactFactoryError::InvalidActor);
        }
        if input.subject.0.trim().is_empty() {
            return Err(ArtifactFactoryError::MissingSubject);
        }
        if input.source.reference.trim().is_empty() {
            return Err(ArtifactFactoryError::InvalidSource);
        }
        if input.stream_key.trim().is_empty() {
            return Err(ArtifactFactoryError::InvalidStreamKey);
        }

        let generated = self.ids.next(&input.schema.artifact_type);
        Ok(Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: input.schema.artifact_type,
            schema_id: input.schema.id,
            schema_version: input.schema.version,
            created_at: self.clock.now_rfc3339(),
            created_by: input.created_by,
            source: input.source,
            trust: input.trust,
            stream_key: input.stream_key,
            subject: Some(input.subject),
            tags: input.tags,
            payload: input.payload,
            provenance: input.provenance,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::core::{
        ActorType, ArtifactId, GeneratedArtifactIds, SchemaId, SourceClass, SourceKind, TrustLevel,
        VersionId,
    };

    use super::*;

    struct FixedClock;

    impl Clock for FixedClock {
        fn now_rfc3339(&self) -> String {
            "2026-07-28T12:00:00Z".into()
        }
    }

    struct FixedIds;

    impl ArtifactIdGenerator for FixedIds {
        fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
            GeneratedArtifactIds {
                artifact_id: ArtifactId(format!("{artifact_type}-1")),
                version_id: VersionId(format!("{artifact_type}-1-v1")),
            }
        }
    }

    fn input() -> ArtifactFactoryInput {
        ArtifactFactoryInput {
            schema: SchemaDefinition {
                id: SchemaId("arcs.input.v1".into()),
                artifact_type: "input".into(),
                version: 1,
                document: json!({}),
            },
            created_by: Actor {
                actor_type: ActorType::Adapter,
                id: "adapter.test".into(),
            },
            source: Source {
                kind: SourceKind::Api,
                reference: "external-1".into(),
            },
            trust: Trust {
                level: TrustLevel::Medium,
                source_class: SourceClass::External,
            },
            stream_key: "stream-1".into(),
            subject: SubjectId("subject-1".into()),
            tags: vec!["adapter:adapter.test".into()],
            payload: json!({"raw_text": "test"}),
            provenance: None,
        }
    }

    #[test]
    fn creates_internal_ids_and_timestamp() {
        let mut ids = FixedIds;
        let mut factory = ArtifactFactory::new(&FixedClock, &mut ids);

        let artifact = factory.create(input()).unwrap();

        assert_eq!(artifact.artifact_id, ArtifactId("input-1".into()));
        assert_eq!(artifact.version_id, VersionId("input-1-v1".into()));
        assert_eq!(artifact.created_at, "2026-07-28T12:00:00Z");
        assert_eq!(artifact.schema_version, 1);
    }

    #[test]
    fn rejects_missing_subject_before_allocating_an_artifact() {
        let mut ids = FixedIds;
        let mut factory = ArtifactFactory::new(&FixedClock, &mut ids);
        let mut invalid = input();
        invalid.subject = SubjectId(" ".into());

        assert_eq!(
            factory.create(invalid),
            Err(ArtifactFactoryError::MissingSubject)
        );
    }
}

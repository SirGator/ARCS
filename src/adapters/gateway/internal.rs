//! Use Case für vom vertrauenswürdigen ARCS-Host erzeugte Core-Artifacte.
//!
//! Externe Adapter verwenden diesen Pfad nicht. Er dient der Runtime etwa zum
//! Anlegen eines DataRequest-Templates, ohne den technischen Store öffentlich
//! beschreibbar zu machen.

use serde_json::Value;

use super::{AdapterGateway, AdapterGatewayError};
use crate::core::{
    Actor, ActorType, Artifact, Provenance, SchemaId, Source, SourceClass, SourceKind, SubjectId,
    Trust, TrustLevel, VersionId,
};

/// Fachlicher Inhalt eines Core-erzeugten Artifacts.
///
/// Autoritätsrelevante Envelope-Felder wie IDs, Typ, Zeit, Actor und Trust
/// bleiben bewusst außerhalb dieses DTOs und werden vom Gateway gesetzt.
#[derive(Debug, Clone, PartialEq)]
pub struct InternalArtifactSubmission {
    pub schema_id: SchemaId,
    pub subject: Option<SubjectId>,
    pub stream_key: String,
    pub internal_reference: String,
    pub tags: Vec<String>,
    pub payload: Value,
    pub parent_versions: Vec<VersionId>,
}

impl AdapterGateway<'_> {
    /// Persistiert ein validiertes, vom ARCS-Host abgeleitetes Event.
    ///
    /// Der Aufrufer ist Teil der vertrauenswürdigen Runtime, nicht eines
    /// Adapterprozesses. Auch hier bleiben Schema- und Referenzprüfung sowie
    /// Core-generierte Identitäten zwingend.
    pub fn record_internal(
        &mut self,
        submission: InternalArtifactSubmission,
    ) -> Result<Artifact, AdapterGatewayError> {
        if submission.stream_key.trim().is_empty()
            || submission.internal_reference.trim().is_empty()
        {
            return Err(AdapterGatewayError::InvalidInternalSubmission);
        }

        for parent in &submission.parent_versions {
            if self.store.get(parent)?.is_none() {
                return Err(AdapterGatewayError::MissingInputArtifact(parent.clone()));
            }
        }

        self.schemas
            .validate(&submission.schema_id, &submission.payload)
            .map_err(AdapterGatewayError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(&submission.schema_id)
            .ok_or_else(|| {
                AdapterGatewayError::MissingRegisteredSchema(submission.schema_id.clone())
            })?
            .clone();
        let generated = self.ids.next(&definition.artifact_type);
        let artifact = Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.runtime".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: submission.internal_reference,
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: submission.stream_key,
            subject: submission.subject,
            tags: submission.tags,
            payload: submission.payload,
            provenance: Some(Provenance {
                parents: submission
                    .parent_versions
                    .iter()
                    .map(|version| version.0.clone())
                    .collect(),
                rules_applied: vec![
                    "adapter_gateway.internal_schema_validated".into(),
                    "adapter_gateway.internal_envelope_assigned".into(),
                ],
                models_used: vec![],
                transform: Some("arcs.runtime".into()),
            }),
        };

        self.store.append(&artifact, self.schemas)?;
        Ok(artifact)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::adapters::{ArtifactIdGenerator, Clock, GeneratedArtifactIds};
    use crate::core::{ArtifactId, SchemaRegistry};
    use crate::store::SqliteArtifactStore;

    struct FixedClock;

    impl Clock for FixedClock {
        fn now_rfc3339(&self) -> String {
            "2026-07-27T12:00:00Z".into()
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

    #[test]
    fn core_fields_cannot_be_supplied_by_the_host_submission() {
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let mut gateway = AdapterGateway::new(
            &mut schemas,
            &store,
            Box::new(FixedClock),
            Box::new(FixedIds),
        );

        let artifact = gateway
            .record_internal(InternalArtifactSubmission {
                schema_id: SchemaId("arcs.input.v1".into()),
                subject: Some(SubjectId("runtime/test".into())),
                stream_key: "runtime:test".into(),
                internal_reference: "rule:test".into(),
                tags: vec!["purpose:test".into()],
                payload: json!({"raw_text": "known request template"}),
                parent_versions: vec![],
            })
            .unwrap();

        assert_eq!(artifact.created_by.actor_type, ActorType::System);
        assert_eq!(artifact.trust.level, TrustLevel::High);
        assert_eq!(artifact.source.kind, SourceKind::Internal);
        assert_eq!(store.get(&artifact.version_id).unwrap(), Some(artifact));
    }

    #[test]
    fn missing_parent_fails_before_any_artifact_is_written() {
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let mut gateway = AdapterGateway::new(
            &mut schemas,
            &store,
            Box::new(FixedClock),
            Box::new(FixedIds),
        );

        let result = gateway.record_internal(InternalArtifactSubmission {
            schema_id: SchemaId("arcs.input.v1".into()),
            subject: None,
            stream_key: "runtime:test".into(),
            internal_reference: "rule:test".into(),
            tags: vec![],
            payload: json!({"raw_text": "derived"}),
            parent_versions: vec![VersionId("missing-v1".into())],
        });

        assert!(matches!(
            result,
            Err(AdapterGatewayError::MissingInputArtifact(version))
                if version == VersionId("missing-v1".into())
        ));
        assert!(store.is_empty().unwrap());
    }
}

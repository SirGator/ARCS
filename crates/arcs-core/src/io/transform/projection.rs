//! Typed projection of a trusted caller's explicit field selection.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{TransformError, service::derived_trust};
use crate::adapters::ProducerClass;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, Clock, Provenance, SchemaId,
    SchemaRegistry, Source, SourceKind, SubjectId, VersionId, validate_artifact,
};
use crate::runtime::{InvocationKind, deterministic_invocation_id};
use crate::store::{SqliteArtifactStore, database::Idempotency, relation_kinds};

pub const PROJECTION_PUBLISHER: &str = "arcs.transform.projection";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformInputProjection {
    pub schema_id: SchemaId,
    pub payload_fields: Vec<String>,
}

impl TransformInputProjection {
    pub fn validate(&self, schemas: &SchemaRegistry) -> Result<(), TransformError> {
        let schema = schemas
            .get(&self.schema_id)
            .ok_or_else(|| TransformError::MissingSchema(self.schema_id.clone()))?;
        if schema.artifact_type != "projection" {
            return Err(TransformError::InvalidProjectionPolicy(
                "projection requires a dedicated arcs.projection schema",
            ));
        }
        let mut seen = HashSet::new();
        if !(1..=64).contains(&self.payload_fields.len())
            || self.payload_fields.iter().any(|field| {
                field.trim().is_empty()
                    || field.len() > 128
                    || field.chars().any(char::is_control)
                    || !seen.insert(field)
            })
        {
            return Err(TransformError::InvalidProjectionPolicy(
                "projection requires 1..64 distinct nonempty field names",
            ));
        }
        Ok(())
    }

    /// Publish or replay an immutable scoped view before calling a Transform adapter.
    pub fn materialize(
        &self,
        source_version: &VersionId,
        store: &SqliteArtifactStore,
        schemas: &SchemaRegistry,
        clock: &dyn Clock,
    ) -> Result<Artifact, TransformError> {
        self.validate(schemas)?;
        let source = store
            .get(source_version)?
            .ok_or_else(|| TransformError::MissingInput(source_version.clone()))?;
        validate_artifact(&source, schemas).map_err(TransformError::InvalidInput)?;
        let Some(object) = source.payload.as_object() else {
            return Err(TransformError::InvalidProjectionPolicy(
                "projection source must have an object payload",
            ));
        };
        let mut fields = self
            .payload_fields
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        fields.sort_unstable();
        let selected = fields
            .iter()
            .map(|&field| {
                object
                    .get(field)
                    .cloned()
                    .map(|value| (field.to_owned(), value))
                    .ok_or_else(|| TransformError::MissingProjectedField(field.to_owned()))
            })
            .collect::<Result<Map<_, _>, _>>()?;
        let payload = Value::Object(selected);
        schemas
            .validate(&self.schema_id, &payload)
            .map_err(TransformError::InvalidProjectionPayload)?;
        let identity_fields = [&source_version.0[..], &self.schema_id.0[..]]
            .into_iter()
            .chain(fields.iter().copied())
            .collect::<Vec<_>>();
        let key = format!(
            "transform-input-projection:{}",
            deterministic_invocation_id(InvocationKind::Transform, &identity_fields)
        );
        let trust = derived_trust(&source, ProducerClass::System, source.trust.level);
        let actor = Actor {
            id: PROJECTION_PUBLISHER.into(),
            actor_type: ActorType::System,
        };
        let subject = SubjectId(key.clone());
        let origin = Source {
            kind: SourceKind::Internal,
            reference: key.clone(),
        };
        let provenance = Provenance {
            parents: vec![source_version.0.clone()],
            rules_applied: vec!["transform.project_selected_fields".into()],
            ..Default::default()
        };
        let saved = if let Some(saved) = store.find_by_stream_key(&key)? {
            saved
        } else {
            let mut ids = store.durable_id_generator("transform-projection")?;
            let artifact = ArtifactFactory::new(clock, &mut ids).create(ArtifactFactoryInput {
                schema: schemas
                    .get(&self.schema_id)
                    .ok_or_else(|| TransformError::MissingSchema(self.schema_id.clone()))?
                    .clone(),
                created_by: actor.clone(),
                source: origin.clone(),
                trust: trust.clone(),
                stream_key: key.clone(),
                subject: subject.clone(),
                tags: vec![],
                payload: payload.clone(),
                provenance: Some(provenance.clone()),
            })?;
            let identity = serde_json::to_string(&(&identity_fields, &payload))?;
            store
                .append_current_related_once(
                    &artifact,
                    schemas,
                    Idempotency {
                        key: &key,
                        input: &identity,
                    },
                    &[(source_version.clone(), relation_kinds::derived_from())],
                )?
                .artifact
        };
        validate_artifact(&saved, schemas).map_err(TransformError::InvalidInput)?;
        if saved.version != 1
            || saved.schema_id != self.schema_id
            || saved.created_by != actor
            || saved.source != origin
            || saved.trust != trust
            || saved.stream_key != key
            || saved.subject.as_ref() != Some(&subject)
            || saved.payload != payload
            || saved.provenance.as_ref() != Some(&provenance)
        {
            return Err(TransformError::ProjectionIdentityChanged);
        }
        Ok(saved)
    }
}

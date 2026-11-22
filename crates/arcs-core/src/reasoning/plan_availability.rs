//! Immutable availability observations for one planner request generation.
use super::{ContextSelection, ReasoningError, ReasoningMethod, selection::ReasonerAvailability};
use crate::adapters::CapabilityRef;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, Provenance, SchemaId,
    SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, SystemClock, Trust, TrustLevel,
    VersionId, validate_artifact,
};
use crate::runtime::{InvocationKind, deterministic_invocation_id};
use crate::store::{SqliteArtifactStore, database::Idempotency, relation_kinds};
use serde::Serialize;
use serde_json::json;

pub const SCHEMA: &str = "arcs.plan_template_availability.v1";
pub const PUBLISHER: &str = "arcs.reasoning.plan_availability";

pub struct PlanAvailabilityOwner<'a> {
    pub definition: &'a VersionId,
    pub input: &'a VersionId,
    pub sequence: u64,
    pub revision: u8,
}

#[derive(Serialize)]
pub struct TemplateAvailability<'a> {
    pub id: &'a str,
    pub reasoners: Vec<ReasonerStatus>,
}

#[derive(Serialize)]
pub struct ReasonerStatus {
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub availability: ReasonerAvailability,
}

impl PlanAvailabilityOwner<'_> {
    fn stream(&self) -> String {
        format!(
            "plan-template-availability:{}",
            deterministic_invocation_id(
                InvocationKind::Request,
                &[
                    &self.definition.0,
                    &self.input.0,
                    &self.sequence.to_string(),
                    &self.revision.to_string()
                ],
            )
        )
    }

    pub fn load(
        &self,
        store: &SqliteArtifactStore,
        schemas: &SchemaRegistry,
    ) -> Result<Option<ContextSelection>, ReasoningError> {
        let key = self.stream();
        store
            .find_by_stream_key(&key)?
            .map(|artifact| self.read(artifact, schemas, &key))
            .transpose()
    }

    pub fn record(
        &self,
        store: &SqliteArtifactStore,
        schemas: &SchemaRegistry,
        templates: &[TemplateAvailability<'_>],
    ) -> Result<ContextSelection, ReasoningError> {
        if !(1..=32).contains(&templates.len())
            || templates
                .iter()
                .any(|template| !(1..=8).contains(&template.reasoners.len()))
        {
            return Err(invalid("template availability exceeds planner bounds"));
        }
        let key = self.stream();
        let parents = [self.definition.clone(), self.input.clone()];
        let schema = SchemaId(SCHEMA.into());
        let mut ids = store.durable_id_generator("plan-availability")?;
        let artifact = ArtifactFactory::new(&SystemClock, &mut ids)
            .create(ArtifactFactoryInput {
                schema: schemas
                    .get(&schema)
                    .ok_or(ReasoningError::MissingRegisteredSchema(schema))?
                    .clone(),
                created_by: Actor {
                    actor_type: ActorType::System,
                    id: PUBLISHER.into(),
                },
                source: Source {
                    kind: SourceKind::Internal,
                    reference: key.clone(),
                },
                trust: Trust {
                    source_class: SourceClass::System,
                    level: TrustLevel::High,
                },
                stream_key: key.clone(),
                subject: SubjectId(key.clone()),
                tags: vec![],
                payload: json!({
                    "definition_version_id":self.definition,
                    "input_version_id":self.input,
                    "sequence":self.sequence,
                    "revision":self.revision,
                    "templates":templates,
                }),
                provenance: Some(Provenance {
                    parents: parents.iter().map(|parent| parent.0.clone()).collect(),
                    ..Default::default()
                }),
            })
            .map_err(|error| invalid(format!("cannot record template availability: {error:?}")))?;
        let relations = parents.map(|parent| (parent, relation_kinds::based_on()));
        // The receipt binds the owner, so concurrent observations keep the first commit.
        let saved = store.append_current_related_once(
            &artifact,
            schemas,
            Idempotency {
                key: &key,
                input: &key,
            },
            &relations,
        )?;
        self.read(saved.artifact, schemas, &key)
    }

    fn read(
        &self,
        artifact: Artifact,
        schemas: &SchemaRegistry,
        key: &str,
    ) -> Result<ContextSelection, ReasoningError> {
        validate_artifact(&artifact, schemas)
            .map_err(|_| invalid("invalid template availability receipt"))?;
        if artifact.schema_id.0 != SCHEMA
            || artifact.version != 1
            || artifact.created_by.actor_type != ActorType::System
            || artifact.created_by.id != PUBLISHER
            || artifact.source.kind != SourceKind::Internal
            || artifact.source.reference != key
            || artifact.stream_key != key
            || artifact.subject.as_ref().map(|subject| subject.0.as_str()) != Some(key)
            || artifact.trust.source_class != SourceClass::System
            || artifact.trust.level != TrustLevel::High
            || artifact.payload["definition_version_id"] != self.definition.0
            || artifact.payload["input_version_id"] != self.input.0
            || artifact.payload["sequence"] != self.sequence
            || artifact.payload["revision"] != self.revision
            || artifact.provenance.as_ref().is_none_or(|provenance| {
                !provenance.parents.contains(&self.definition.0)
                    || !provenance.parents.contains(&self.input.0)
            })
        {
            return Err(invalid("template availability identity changed"));
        }
        Ok(ContextSelection {
            version_id: artifact.version_id,
            payload_fields: vec!["revision".into(), "templates".into()],
        })
    }
}

fn invalid(message: impl Into<String>) -> ReasoningError {
    ReasoningError::InvalidReasoningRequest(message.into())
}

use super::*;
use crate::core::{
    Actor, ActorType, ArtifactFactory, ArtifactFactoryInput, Provenance, Source, SourceClass,
    SourceKind, SubjectId, Trust, TrustLevel, validate_artifact,
};
use crate::runtime::{InvocationKind, deterministic_invocation_id};
use crate::store::{database::Idempotency, relation_kinds};

pub(super) fn stream(owner: &SelectionOwner) -> String {
    format!(
        "reasoner-selection:{}",
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                &owner.definition_version.0,
                &owner.input_version.0,
                &owner.task_id
            ]
        )
    )
}

pub(in crate::reasoning) fn validate_owner(
    store: &SqliteArtifactStore,
    owner: &SelectionOwner,
) -> Result<(), ReasoningError> {
    if owner.task_id.is_empty()
        || owner.task_id.len() > 128
        || !owner
            .task_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(invalid("invalid selection task identity"));
    }
    let definition = store
        .get(&owner.definition_version)?
        .ok_or_else(|| invalid("missing selection definition"))?;
    if definition.created_by.actor_type != ActorType::System
        || definition.source.kind != SourceKind::Internal
        || definition.trust.source_class != SourceClass::System
        || definition.trust.level != TrustLevel::High
    {
        return Err(invalid(
            "reasoner selection requires a trusted system definition",
        ));
    }
    if store.get(&owner.input_version)?.is_none() {
        return Err(invalid("missing selection input"));
    }
    Ok(())
}

pub(super) fn read(
    artifact: &Artifact,
    owner: &SelectionOwner,
    policy: &SelectionPolicy,
    schemas: &SchemaRegistry,
) -> Result<SelectedReasoner, ReasoningError> {
    validate_artifact(artifact, schemas)
        .map_err(|_| invalid("invalid reasoner selection artifact"))?;
    if artifact.schema_id.0 != SCHEMA
        || artifact.version != 1
        || artifact.created_by.id != PUBLISHER
        || artifact.created_by.actor_type != ActorType::System
        || artifact.source.kind != SourceKind::Internal
        || artifact.trust.source_class != SourceClass::System
        || artifact.trust.level != TrustLevel::High
        || artifact.stream_key != stream(owner)
        || artifact.subject.as_ref() != Some(&SubjectId(stream(owner)))
        || artifact.provenance.as_ref().is_none_or(|provenance| {
            !provenance.parents.contains(&owner.definition_version.0)
                || !provenance.parents.contains(&owner.input_version.0)
        })
    {
        return Err(invalid("untrusted reasoner selection receipt"));
    }
    let record: SelectionRecord = serde_json::from_value(artifact.payload.clone())?;
    let tier = history::top_tier(&record.observations);
    if record.owner != *owner
        || record.policy != *policy
        || record.observations.len() != policy.choices.len()
        || tier.is_empty()
        || !history::recorded_samples_valid(&record, &tier)
        || record.selected_index
            != history::pick(&tier, &record.observations, policy.history.as_ref())
        || artifact.provenance.as_ref().is_none_or(|provenance| {
            record
                .observations
                .iter()
                .filter_map(|option| option.history.as_ref())
                .flat_map(|sample| &sample.sample_versions)
                .any(|attempt| !provenance.parents.contains(&attempt.0))
        })
    {
        return Err(invalid("reasoner selection identity or choice changed"));
    }
    Ok(SelectedReasoner {
        version_id: artifact.version_id.clone(),
        record,
    })
}

pub(super) fn persist(
    record: &SelectionRecord,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    clock: &dyn Clock,
) -> Result<Artifact, ReasoningError> {
    let key = stream(&record.owner);
    let mut seen = HashSet::new();
    let parents = [
        &record.owner.definition_version,
        &record.owner.input_version,
    ]
    .into_iter()
    .chain(
        record
            .observations
            .iter()
            .filter_map(|option| option.history.as_ref())
            .flat_map(|sample| &sample.sample_versions),
    )
    .filter(|version| seen.insert(*version))
    .cloned()
    .collect::<Vec<_>>();
    let artifact = ArtifactFactory::new(clock, ids)
        .create(ArtifactFactoryInput {
            schema: schemas
                .get(&SchemaId(SCHEMA.into()))
                .ok_or_else(|| ReasoningError::MissingRegisteredSchema(SchemaId(SCHEMA.into())))?
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
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: key.clone(),
            subject: SubjectId(key.clone()),
            tags: vec![],
            payload: serde_json::to_value(record)?,
            provenance: Some(Provenance {
                parents: parents.iter().map(|version| version.0.clone()).collect(),
                rules_applied: vec!["reasoner.selection.v1".into()],
                ..Default::default()
            }),
        })
        .map_err(|_| invalid("cannot construct reasoner selection"))?;
    let identity = serde_json::to_string(&(&record.owner, &record.policy))?;
    let relations = parents
        .into_iter()
        .map(|version| (version, relation_kinds::based_on()))
        .collect::<Vec<_>>();
    Ok(store
        .append_current_related_once(
            &artifact,
            schemas,
            Idempotency {
                key: &key,
                input: &identity,
            },
            &relations,
        )?
        .artifact)
}

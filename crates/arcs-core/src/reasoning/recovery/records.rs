use super::*;
use crate::store::database::Idempotency;

pub(super) fn definition_key(owner: &SelectionOwner, request_id: &str) -> String {
    format!(
        "reasoning-recovery:{}",
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                &owner.definition_version.0,
                &owner.input_version.0,
                &owner.task_id,
                request_id
            ]
        )
    )
}
pub(super) fn step_key(definition: &VersionId, index: usize) -> String {
    format!(
        "reasoning-recovery-step:{}",
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[&definition.0, &index.to_string()]
        )
    )
}
pub(super) fn persist_definition(
    service: &mut RecoveryService<'_>,
    definition: &RecoveryDefinition,
) -> Result<Artifact, ReasoningError> {
    let key = definition_key(&definition.owner, &definition.base_request.request_id);
    let payload = serde_json::to_value(definition)?;
    if let Some(saved) = service.store.find_by_stream_key(&key)? {
        if saved.payload != payload {
            return Err(invalid("recovery definition identity conflict"));
        }
        return Ok(saved);
    }
    let mut parents = vec![
        definition.owner.definition_version.clone(),
        definition.owner.input_version.clone(),
    ];
    parents.extend(
        definition
            .base_request
            .context
            .iter()
            .map(|item| item.version_id.clone()),
    );
    persist(
        service,
        SCHEMA,
        &key,
        payload.clone(),
        &serde_json::to_string(&payload)?,
        parents,
    )
}
pub(super) fn persist_step(
    service: &mut RecoveryService<'_>,
    step: &RecoveryStep,
) -> Result<Artifact, ReasoningError> {
    let key = step_key(&step.definition_version_id, step.index);
    let mut parents = vec![step.definition_version_id.clone()];
    parents.extend(step.previous_step.iter().cloned());
    parents.extend(step.failure_version_id.iter().cloned());
    // Concurrent callers agree on the first committed due time. The failure,
    // previous step and definition are immutable parts of the decision identity.
    let identity = serde_json::to_string(&(
        &step.definition_version_id,
        step.index,
        &step.previous_step,
        &step.failure_version_id,
    ))?;
    persist(
        service,
        STEP_SCHEMA,
        &key,
        serde_json::to_value(step)?,
        &identity,
        parents,
    )
}
fn persist(
    service: &mut RecoveryService<'_>,
    schema: &str,
    key: &str,
    payload: serde_json::Value,
    identity: &str,
    mut parents: Vec<VersionId>,
) -> Result<Artifact, ReasoningError> {
    if serde_json::to_vec(&payload)?.len() > 60_000 {
        return Err(invalid("recovery record exceeds 60000 bytes"));
    }
    parents.sort_by(|a, b| a.0.cmp(&b.0));
    parents.dedup();
    let artifact = ArtifactFactory::new(service.clock, service.ids)
        .create(ArtifactFactoryInput {
            schema: service
                .schemas
                .get(&SchemaId(schema.into()))
                .ok_or_else(|| ReasoningError::MissingRegisteredSchema(SchemaId(schema.into())))?
                .clone(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: key.into(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: key.into(),
            subject: SubjectId(key.into()),
            tags: vec![],
            payload,
            provenance: Some(Provenance {
                parents: parents.iter().map(|id| id.0.clone()).collect(),
                rules_applied: vec!["reasoning.recovery.v1".into()],
                ..Default::default()
            }),
        })
        .map_err(|_| invalid("cannot construct recovery record"))?;
    let relations = parents
        .into_iter()
        .map(|id| (id, relation_kinds::based_on()))
        .collect::<Vec<_>>();
    Ok(service
        .store
        .append_current_related_once(
            &artifact,
            service.schemas,
            Idempotency {
                key,
                input: identity,
            },
            &relations,
        )?
        .artifact)
}

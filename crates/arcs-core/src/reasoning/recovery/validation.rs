use super::*;

pub(super) fn definition(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    value: &RecoveryDefinition,
) -> Result<(), ReasoningError> {
    super::super::selection::records::validate_owner(store, &value.owner)?;
    let base = &value.base_request;
    value
        .policy
        .validate(registry, schemas, &base.target_schema_id)?;
    SelectionPolicy {
        target_schema_id: base.target_schema_id.clone(),
        query_text: String::new(),
        choices: vec![choice(value, 0)],
    }
    .validate(registry, schemas)?;
    super::super::selection::request_selection(registry, schemas, store, base)?;
    if base.request_id.is_empty()
        || base.request_id.len() > 256
        || base.request_id.chars().any(char::is_control)
        || base.objective.trim().is_empty()
        || base.constraints.get(CONSTRAINT).is_some()
        || !base
            .context
            .iter()
            .any(|item| item.version_id == value.owner.input_version)
        || serde_json::to_vec(value)?.len() > 60_000
    {
        return Err(invalid("invalid recovery base request or owner context"));
    }
    Ok(())
}
fn trusted(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    version: &VersionId,
    schema: &str,
    key: Option<&str>,
) -> Result<Artifact, ReasoningError> {
    let artifact = store
        .get(version)?
        .ok_or_else(|| invalid("missing recovery record"))?;
    validate_artifact(&artifact, schemas)
        .map_err(|_| invalid("invalid recovery record envelope"))?;
    if artifact.schema_id.0 != schema
        || artifact.version != 1
        || artifact.created_by.id != PUBLISHER
        || artifact.created_by.actor_type != ActorType::System
        || artifact.source.kind != SourceKind::Internal
        || artifact.trust.source_class != SourceClass::System
        || artifact.trust.level != TrustLevel::High
        || artifact.subject.as_ref() != Some(&SubjectId(artifact.stream_key.clone()))
        || artifact.source.reference != artifact.stream_key
        || key.is_some_and(|key| artifact.stream_key != key)
        || serde_json::to_vec(&artifact.payload)?.len() > 60_000
    {
        return Err(invalid("untrusted recovery record"));
    }
    if store
        .find_by_stream_key(&artifact.stream_key)?
        .is_none_or(|saved| saved.version_id != artifact.version_id)
    {
        return Err(invalid("noncanonical recovery record"));
    }
    Ok(artifact)
}
pub(super) fn load(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    version: &VersionId,
) -> Result<(RecoveryDefinition, RecoveryStep), ReasoningError> {
    let artifact = trusted(store, schemas, version, STEP_SCHEMA, None)?;
    let step: RecoveryStep = serde_json::from_value(artifact.payload)?;
    let value = load_definition(registry, schemas, store, &step.definition_version_id)?;
    if step.index > value.policy.alternatives.len() {
        return Err(invalid("recovery index changed"));
    }
    let mut current = step.clone();
    let mut current_version = version.clone();
    for expected in (0..=step.index).rev() {
        let artifact = trusted(
            store,
            schemas,
            &current_version,
            STEP_SCHEMA,
            Some(&records::step_key(&step.definition_version_id, expected)),
        )?;
        if current.definition_version_id != step.definition_version_id
            || current.index != expected
            || current.not_before_unix_ms > i64::MAX as u64
        {
            return Err(invalid("invalid recovery step chain"));
        }
        let parents = &artifact
            .provenance
            .as_ref()
            .ok_or_else(|| invalid("missing recovery provenance"))?
            .parents;
        if !parents.contains(&step.definition_version_id.0) {
            return Err(invalid("missing recovery definition provenance"));
        }
        if expected == 0 {
            if current.previous_step.is_some()
                || current.failure_version_id.is_some()
                || current.not_before_unix_ms != 0
            {
                return Err(invalid("invalid initial recovery step"));
            }
            break;
        }
        let previous_version = current
            .previous_step
            .as_ref()
            .ok_or_else(|| invalid("missing previous recovery step"))?;
        let failure_version = current
            .failure_version_id
            .as_ref()
            .ok_or_else(|| invalid("missing preceding recovery failure"))?;
        if !parents.contains(&previous_version.0) || !parents.contains(&failure_version.0) {
            return Err(invalid("missing recovery failure provenance"));
        }
        let previous_artifact = trusted(
            store,
            schemas,
            previous_version,
            STEP_SCHEMA,
            Some(&records::step_key(
                &step.definition_version_id,
                expected - 1,
            )),
        )?;
        let previous: RecoveryStep = serde_json::from_value(previous_artifact.payload)?;
        if previous.index != expected - 1
            || previous.definition_version_id != step.definition_version_id
            || previous
                .not_before_unix_ms
                .checked_add(value.policy.delay_ms)
                .is_none_or(|minimum| current.not_before_unix_ms < minimum)
        {
            return Err(invalid("invalid recovery predecessor or delay"));
        }
        let failure = store
            .get(failure_version)?
            .ok_or_else(|| invalid("missing recovery failure artifact"))?;
        let outcome = failure_for_step(
            store,
            schemas,
            &value,
            &previous,
            previous_version,
            &failure,
        )?;
        if !eligible(&outcome.outcome) {
            return Err(invalid(
                "recovery requires a retryable adapter timeout or unavailability",
            ));
        }
        current_version = previous_version.clone();
        current = previous;
    }
    Ok((value, step))
}
pub(super) fn failure_for_step(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    definition: &RecoveryDefinition,
    step: &RecoveryStep,
    version: &VersionId,
    artifact: &Artifact,
) -> Result<super::super::attempts::ReasoningAttempt, ReasoningError> {
    use crate::store::database::MetadataField;
    let record = super::super::attempts::read(store, schemas, artifact)?;
    let expected = request(definition, step, version);
    let audit = store
        .metadata_projection(
            &record.request_version_id,
            &[MetadataField::value(
                "step",
                "$.payload.invocation.constraints.arcs_reasoning_recovery",
            )],
            1024,
        )?
        .ok_or_else(|| invalid("missing failed recovery request"))?;
    if record.request_id != expected.request_id
        || record.invocation_id != invocation_id(&expected)
        || record.capability != expected.capability
        || record.method != expected.method
        || record.budget != expected.budget
        || audit.fields["step"] != serde_json::to_value(version)?
        || !matches!(
            record.outcome,
            super::super::attempts::AttemptOutcome::Failed { .. }
        )
    {
        return Err(invalid("failure does not belong to this recovery step"));
    }
    Ok(record)
}

pub(super) fn load_definition(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    version: &VersionId,
) -> Result<RecoveryDefinition, ReasoningError> {
    let artifact = trusted(store, schemas, version, SCHEMA, None)?;
    let value: RecoveryDefinition = serde_json::from_value(artifact.payload)?;
    definition(registry, schemas, store, &value)?;
    let parents = artifact
        .provenance
        .as_ref()
        .ok_or_else(|| invalid("missing recovery definition provenance"))?;
    if !parents.parents.contains(&value.owner.definition_version.0)
        || !parents.parents.contains(&value.owner.input_version.0)
    {
        return Err(invalid("missing recovery owner provenance"));
    }
    if artifact.stream_key != records::definition_key(&value.owner, &value.base_request.request_id)
    {
        return Err(invalid("recovery definition identity changed"));
    }
    Ok(value)
}

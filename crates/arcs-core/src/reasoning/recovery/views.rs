use super::*;

/// Resolve the actual producer for a completed root/plan request, while keeping
/// its original task binding and policy authoritative for joins and admission.
pub fn completed_choice(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    request_version: &VersionId,
    owner: &SelectionOwner,
    policy: &RecoveryPolicy,
    base_request_id: &str,
) -> Result<ReasonerChoice, ReasoningError> {
    use crate::store::database::MetadataField;
    let audit = store
        .metadata_projection(
            request_version,
            &[
                MetadataField::value(
                    "step",
                    "$.payload.invocation.constraints.arcs_reasoning_recovery",
                ),
                MetadataField::value("request_id", "$.payload.invocation.request_id"),
                MetadataField::value("capability", "$.payload.invocation.capability"),
                MetadataField::value("method", "$.payload.invocation.method"),
                MetadataField::value("budget", "$.payload.invocation.budget"),
            ],
            32_768,
        )?
        .ok_or_else(|| invalid("missing completed recovery request"))?;
    if audit.schema_id.0 != "arcs.reasoning_request.v2"
        || audit.created_by.id != "arcs.reasoning.v2"
        || audit.created_by.actor_type != ActorType::System
        || audit.source.kind != SourceKind::Internal
        || audit.trust.level != TrustLevel::High
        || audit.trust.source_class != SourceClass::System
    {
        return Err(invalid("untrusted completed recovery request"));
    }
    let version: VersionId = serde_json::from_value(audit.fields["step"].clone())?;
    let (definition, step) = validation::load(registry, schemas, store, &version)?;
    let expected = request(&definition, &step, &version);
    let completed =
        InvocationService::new(store, schemas, &SystemClock).lookup(&invocation_id(&expected))?;
    if definition.owner != *owner
        || definition.policy != *policy
        || definition.base_request.request_id != base_request_id
        || audit.fields["request_id"] != expected.request_id
        || audit.fields["capability"] != serde_json::to_value(&expected.capability)?
        || audit.fields["method"] != serde_json::to_value(expected.method)?
        || audit.fields["budget"] != serde_json::to_value(&expected.budget)?
        || completed.is_none_or(|state| {
            state.status != InvocationStatus::Succeeded || state.input_version != *request_version
        })
    {
        return Err(invalid(
            "completed recovery request changed its task or choice",
        ));
    }
    Ok(choice(&definition, step.index))
}

/// Current bounded decision metadata, without private constraints or context.
#[derive(Debug, Serialize)]
pub struct RecoveryStatus {
    pub definition_version_id: VersionId,
    pub step_version_id: VersionId,
    pub attempt_number: usize,
    pub max_attempts: usize,
    pub not_before_unix_ms: u64,
    pub selected: ReasonerChoice,
    pub preceding_failure: Option<VersionId>,
    pub invocation_status: Option<InvocationStatus>,
}
pub fn status(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    definition_version: &VersionId,
    owner: &SelectionOwner,
    policy: &RecoveryPolicy,
) -> Result<RecoveryStatus, ReasoningError> {
    let definition = validation::load_definition(registry, schemas, store, definition_version)?;
    if definition.owner != *owner || definition.policy != *policy {
        return Err(invalid("recovery report task or policy changed"));
    }
    let mut latest = None;
    for index in 0..=definition.policy.alternatives.len() {
        match store.find_by_stream_key(&records::step_key(definition_version, index))? {
            Some(artifact) => latest = Some(artifact.version_id),
            None => break,
        }
    }
    let version = latest.ok_or_else(|| invalid("recovery has no initial step"))?;
    let (_, step) = validation::load(registry, schemas, store, &version)?;
    let request = request(&definition, &step, &version);
    let state =
        InvocationService::new(store, schemas, &SystemClock).lookup(&invocation_id(&request))?;
    Ok(RecoveryStatus {
        definition_version_id: definition_version.clone(),
        step_version_id: version,
        attempt_number: step.index + 1,
        max_attempts: definition.policy.alternatives.len() + 1,
        not_before_unix_ms: step.not_before_unix_ms,
        selected: choice(&definition, step.index),
        preceding_failure: step.failure_version_id,
        invocation_status: state.map(|state| state.status),
    })
}

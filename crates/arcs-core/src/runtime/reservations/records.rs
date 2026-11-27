use super::*;
use crate::core::security::SecurityProfile;
use crate::core::{
    ArtifactId, Provenance, SchemaId, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, validate_artifact,
};
use crate::runtime::{InvocationKind, deterministic_invocation_id};

pub(super) fn subject(resource: &str) -> SubjectId {
    SubjectId(format!("exclusive-resource:{resource}"))
}

pub(super) fn parse(artifact: &Artifact) -> Result<Reservation, ReservationError> {
    if artifact.schema_id.0 != SCHEMA
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PUBLISHER
        || artifact.source.kind != SourceKind::Internal
    {
        return Err(invalid("untrusted resource reservation"));
    }
    let mut value = artifact.payload.clone();
    value["version_id"] = serde_json::json!(artifact.version_id);
    let record: Reservation = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
    let id = deterministic_invocation_id(
        InvocationKind::Request,
        &["exclusive-resource", &record.resource],
    );
    if artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&subject(&record.resource))
    {
        return Err(invalid("reservation artifact identity changed"));
    }
    Ok(record)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make(
    resource: &str,
    owner: &ReservationOwner,
    state: ReservationState,
    previous: Option<&Artifact>,
    actor: &Actor,
    reason: &str,
    evidence: &[VersionId],
    clock: &dyn Clock,
    schemas: &SchemaRegistry,
) -> Result<Artifact, ReservationError> {
    let id =
        deterministic_invocation_id(InvocationKind::Request, &["exclusive-resource", resource]);
    let version = previous.map_or(1, |a| a.version + 1);
    if version > i64::MAX as u64 {
        return Err(invalid("reservation revision exhausted"));
    }
    let schema = schemas
        .get(&SchemaId(SCHEMA.into()))
        .ok_or_else(|| invalid("missing reservation schema"))?;
    let mut parents = BTreeSet::from([
        owner.definition_version.0.clone(),
        owner.input_version.0.clone(),
    ]);
    if let Some(a) = previous {
        parents.insert(a.version_id.0.clone());
    }
    parents.extend(evidence.iter().map(|id| id.0.clone()));
    let artifact = Artifact {
        artifact_id: ArtifactId(id.clone()),
        version_id: VersionId(format!("{id}:v{version}")),
        version,
        artifact_type: schema.artifact_type.clone(),
        schema_id: schema.id.clone(),
        schema_version: schema.version,
        created_at: clock.now_rfc3339(),
        created_by: Actor {
            actor_type: ActorType::System,
            id: PUBLISHER.into(),
        },
        source: Source {
            kind: SourceKind::Internal,
            reference: resource.into(),
        },
        trust: Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        stream_key: format!("exclusive-resource:{resource}"),
        subject: Some(subject(resource)),
        tags: vec![],
        payload: serde_json::json!({"resource":resource,"owner":owner,"state":state,"decision_by":actor,"reason":reason,"evidence":evidence}),
        provenance: Some(Provenance {
            parents: parents.into_iter().collect(),
            rules_applied: vec!["runtime.exclusive_control".into()],
            models_used: vec![],
            transform: Some("runtime.reservation".into()),
        }),
        security: SecurityProfile::default(),
    };
    validate_artifact(&artifact, schemas)
        .map_err(|e| invalid(format!("invalid reservation artifact: {e:?}")))?;
    Ok(artifact)
}

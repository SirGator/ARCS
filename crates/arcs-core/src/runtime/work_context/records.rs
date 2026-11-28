//! Erzeugung und Lesen des persistierten Arbeitskontexts.

use super::*;
use crate::core::security::{Classification, SecurityProfile};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, Provenance, SchemaId, Source, SourceKind, SubjectId,
    Trust, TrustLevel, validate_artifact,
};

/// Fachliches Subject des Kontexts; zugleich der Current-State-Schlüssel.
pub(super) fn subject(runtime_id: &str, task_sequence: u64) -> SubjectId {
    SubjectId(format!("work-context:{runtime_id}:{task_sequence}"))
}

/// Stream, in dem alle Versionen desselben Kontexts liegen.
pub(super) fn stream_key(runtime_id: &str, task_sequence: u64) -> String {
    format!("work-context:{runtime_id}:{task_sequence}")
}

/// Deterministische Identität eines Arbeitskontexts.
///
/// Die Identität hängt nur von Runtime und Task ab. Damit findet ein
/// Neustart denselben Kontext wieder, ohne einen zweiten zu erzeugen.
pub(super) fn context_id(runtime_id: &str, task_sequence: u64) -> String {
    format!(
        "work-context:{:016x}",
        hash(&[runtime_id, &task_sequence.to_string()])
    )
}

/// Stabiler Fingerprint für Kontextidentitäten.
fn hash(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in part.as_bytes().iter().chain([0xff].iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

/// Rekonstruiert den Payload aus einem gelesenen Kontext.
pub(super) fn payload(context: &WorkContext) -> Result<WorkContextPayload, WorkContextError> {
    Ok(WorkContextPayload {
        context_id: context.context_id.clone(),
        runtime_id: context.runtime_id.clone(),
        task_sequence: context.task_sequence,
        route_id: context.route_id.clone(),
        goal_id: context.goal_id.clone(),
        plan_revision: context.plan_revision.clone(),
        state: context.state,
        waiting_for: context.waiting_for.clone(),
        artifact_versions: context.artifact_versions.clone(),
        authority: context.authority.clone(),
        memory: context.memory.clone(),
        priority: context.priority,
        budget: context.budget,
        operations: context.operations.clone(),
    })
}

/// Liest einen Arbeitskontext aus einem gespeicherten Artefakt.
///
/// Der Abgleich von Schema, Erzeuger, Identität und Subject schützt vor
/// fremden oder umgebauten Artefakten, die denselben Vertrag tragen.
pub(super) fn parse(artifact: &Artifact) -> Result<WorkContext, WorkContextError> {
    if artifact.schema_id.0 != SCHEMA
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PUBLISHER
        || artifact.source.kind != SourceKind::Internal
    {
        return Err(invalid("untrusted work context"));
    }
    let payload: WorkContextPayload =
        serde_json::from_value(artifact.payload.clone()).map_err(|e| invalid(e.to_string()))?;
    let id = context_id(&payload.runtime_id, payload.task_sequence);
    if artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&subject(&payload.runtime_id, payload.task_sequence))
    {
        return Err(invalid("work context identity changed"));
    }
    Ok(WorkContext {
        context_id: payload.context_id,
        runtime_id: payload.runtime_id,
        task_sequence: payload.task_sequence,
        route_id: payload.route_id,
        goal_id: payload.goal_id,
        plan_revision: payload.plan_revision,
        state: payload.state,
        waiting_for: payload.waiting_for,
        artifact_versions: payload.artifact_versions,
        authority: payload.authority,
        memory: payload.memory,
        priority: payload.priority,
        budget: payload.budget,
        operations: payload.operations,
        version: artifact.version,
        version_id: artifact.version_id.clone(),
        classification: artifact.security.effective(),
    })
}

/// Erzeugt die nächste Version eines Arbeitskontexts.
pub(super) fn make(
    payload: &WorkContextPayload,
    version: u64,
    previous: Option<&VersionId>,
    classification: Classification,
    clock: &dyn Clock,
    schemas: &SchemaRegistry,
) -> Result<Artifact, WorkContextError> {
    let schema = schemas
        .get(&SchemaId(SCHEMA.into()))
        .ok_or_else(|| invalid("missing work context schema"))?;
    let id = context_id(&payload.runtime_id, payload.task_sequence);
    let mut parents = Vec::new();
    if let Some(previous) = previous {
        parents.push(previous.0.clone());
    }
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
            reference: format!("{}:{}", payload.runtime_id, payload.task_sequence),
        },
        trust: Trust {
            level: TrustLevel::High,
            source_class: crate::core::SourceClass::System,
        },
        stream_key: stream_key(&payload.runtime_id, payload.task_sequence),
        subject: Some(subject(&payload.runtime_id, payload.task_sequence)),
        tags: vec![],
        payload: serde_json::to_value(payload).map_err(|e| invalid(e.to_string()))?,
        provenance: Some(Provenance {
            parents,
            rules_applied: vec!["runtime.work_context".into()],
            models_used: vec![],
            transform: Some("runtime.work_context".into()),
        }),
        security: SecurityProfile::declared(classification),
    };
    validate_artifact(&artifact, schemas)
        .map_err(|e| invalid(format!("invalid work context artifact: {e:?}")))?;
    Ok(artifact)
}

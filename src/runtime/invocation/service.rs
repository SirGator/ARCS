use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, Clock, Provenance, SchemaId, SchemaRegistry, Source,
    SourceClass, SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError};

use super::InvocationStatus;

const INVOCATION_SCHEMA_ID: &str = "arcs.invocation.v1";
const RETRYABLE_ERROR_PREFIX: &str = "retryable:";
const PERMANENT_ERROR_PREFIX: &str = "permanent:";

/// Art eines deduplizierten Runtime-Aufrufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Request,
    Reasoning,
    Execution,
    Output,
}

/// Unveränderliche Identität eines geplanten Aufrufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationSpec {
    pub invocation_id: String,
    pub kind: InvocationKind,
    pub capability: String,
    pub input_version: VersionId,
    pub input_fingerprint: String,
}

/// Aus einem Current-State-Artifact gelesener Invocation-Zustand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationState {
    pub invocation_id: String,
    pub kind: InvocationKind,
    pub capability: String,
    pub input_version: VersionId,
    pub input_fingerprint: String,
    pub status: InvocationStatus,
    pub result_version: Option<VersionId>,
    pub error: Option<String>,
    /// Nur ein explizit als temporär klassifizierter Fehler darf nach einer
    /// Wiederherstellung in einen neuen Dispatch übergehen.
    pub retryable: bool,
    pub created_at: String,
    pub updated_at: String,
    artifact_id: ArtifactId,
    artifact_version: u64,
}

/// Fehler an der persistierten Invocation-Grenze.
#[derive(Debug)]
pub enum InvocationError {
    Store(StoreError),
    InvalidState(String),
    IdentityConflict(String),
    NotRunnable(InvocationStatus),
    InvalidTransition {
        from: InvocationStatus,
        to: InvocationStatus,
    },
    MissingResult,
}

impl From<StoreError> for InvocationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Gemeinsame, Store-basierte Idempotenz für externe Runtime-Aufrufe.
pub struct InvocationService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}

impl<'a> InvocationService<'a> {
    pub fn new(
        store: &'a SqliteArtifactStore,
        schemas: &'a SchemaRegistry,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            store,
            schemas,
            clock,
        }
    }

    pub fn lookup(&self, invocation_id: &str) -> Result<Option<InvocationState>, InvocationError> {
        let subject = SubjectId(invocation_subject(invocation_id));
        let schema = SchemaId(INVOCATION_SCHEMA_ID.into());
        self.store
            .current(&subject, &schema)?
            .map(parse_state)
            .transpose()
    }

    /// Prüft, ob ein persistierter State exakt zu einem aktuellen Aufruf gehört.
    pub fn assert_identity(
        &self,
        state: &InvocationState,
        spec: &InvocationSpec,
    ) -> Result<(), InvocationError> {
        assert_same_identity(state, spec)
    }

    /// Persistiert `prepared`, wenn die Korrelation noch unbekannt ist.
    pub fn prepare(&self, spec: InvocationSpec) -> Result<InvocationState, InvocationError> {
        if let Some(state) = self.lookup(&spec.invocation_id)? {
            self.assert_identity(&state, &spec)?;
            return Ok(state);
        }
        let now = self.clock.now_rfc3339();
        let state = new_state(spec, now);
        let artifact = state_artifact(&state, self.schemas)?;
        self.store.append_current(&artifact, self.schemas)?;
        Ok(state)
    }

    /// Bereitet einen Invocation-State und ein Audit-Ereignis atomar vor.
    pub fn prepare_with_event(
        &self,
        spec: InvocationSpec,
        event: &Artifact,
    ) -> Result<InvocationState, InvocationError> {
        if let Some(state) = self.lookup(&spec.invocation_id)? {
            self.assert_identity(&state, &spec)?;
            return Ok(state);
        }
        let now = self.clock.now_rfc3339();
        let state = new_state(spec, now);
        let artifact = state_artifact(&state, self.schemas)?;
        self.store
            .append_current_with_event(&artifact, event, self.schemas, &[])?;
        Ok(state)
    }

    /// Markiert einen vorbereiteten oder wiederhergestellten Invocation vor
    /// seinem externen Aufruf.
    pub fn dispatch(&self, state: &InvocationState) -> Result<InvocationState, InvocationError> {
        self.transition(state, InvocationStatus::Dispatched, None, None, false)
    }

    /// Stellt eine vor dem Ergebnis unterbrochene Invocation sicher wieder her.
    ///
    /// Ein gefundenes `dispatched` bedeutet, dass die externe Wirkung bereits
    /// eingetreten sein könnte. Es wird deshalb zuerst auditierbar zu
    /// `unknown` und darf anschließend nur mit derselben `invocation_id`
    /// erneut an einen deduplizierenden Microservice gesendet werden.
    pub fn recover(&self, state: &InvocationState) -> Result<InvocationState, InvocationError> {
        match state.status {
            InvocationStatus::Prepared | InvocationStatus::Unknown => Ok(state.clone()),
            InvocationStatus::Dispatched => {
                self.transition(state, InvocationStatus::Unknown, None, None, false)
            }
            InvocationStatus::Failed if state.retryable => {
                self.transition(state, InvocationStatus::Prepared, None, None, false)
            }
            status => Err(InvocationError::NotRunnable(status)),
        }
    }

    /// Hält einen Fehler nach einem externen Versuch inklusive Retryability fest.
    pub fn fail(
        &self,
        state: &InvocationState,
        error: impl Into<String>,
        retryable: bool,
    ) -> Result<InvocationState, InvocationError> {
        self.transition(
            state,
            InvocationStatus::Failed,
            None,
            Some(error.into()),
            retryable,
        )
    }

    /// Atomarer Commit für ein ereignisförmiges Resultat und `succeeded`.
    pub fn succeed_with_event(
        &self,
        state: &InvocationState,
        result: &Artifact,
        relations: &[(VersionId, crate::store::RelationKind)],
    ) -> Result<InvocationState, InvocationError> {
        ensure_transition(state.status, InvocationStatus::Succeeded, state.retryable)?;
        let next = self.next_state(
            state,
            InvocationStatus::Succeeded,
            Some(result.version_id.clone()),
            None,
            false,
        );
        let artifact = state_artifact(&next, self.schemas)?;
        self.store
            .append_current_with_event(&artifact, result, self.schemas, relations)?;
        Ok(next)
    }

    /// Atomarer Commit für ein Current-State-Resultat und `succeeded`.
    pub fn succeed_with_current(
        &self,
        state: &InvocationState,
        result: &Artifact,
        relations: &[(VersionId, crate::store::RelationKind)],
    ) -> Result<InvocationState, InvocationError> {
        ensure_transition(state.status, InvocationStatus::Succeeded, state.retryable)?;
        let next = self.next_state(
            state,
            InvocationStatus::Succeeded,
            Some(result.version_id.clone()),
            None,
            false,
        );
        let artifact = state_artifact(&next, self.schemas)?;
        self.store
            .append_two_current_related(result, relations, &artifact, self.schemas)?;
        Ok(next)
    }

    fn transition(
        &self,
        state: &InvocationState,
        status: InvocationStatus,
        result_version: Option<VersionId>,
        error: Option<String>,
        retryable: bool,
    ) -> Result<InvocationState, InvocationError> {
        ensure_transition(state.status, status, state.retryable)?;
        let next = self.next_state(state, status, result_version, error, retryable);
        let artifact = state_artifact(&next, self.schemas)?;
        self.store.append_current(&artifact, self.schemas)?;
        Ok(next)
    }

    fn next_state(
        &self,
        state: &InvocationState,
        status: InvocationStatus,
        result_version: Option<VersionId>,
        error: Option<String>,
        retryable: bool,
    ) -> InvocationState {
        InvocationState {
            invocation_id: state.invocation_id.clone(),
            kind: state.kind,
            capability: state.capability.clone(),
            input_version: state.input_version.clone(),
            input_fingerprint: state.input_fingerprint.clone(),
            status,
            result_version,
            error,
            retryable,
            created_at: state.created_at.clone(),
            updated_at: self.clock.now_rfc3339(),
            artifact_id: state.artifact_id.clone(),
            artifact_version: state.artifact_version + 1,
        }
    }
}

/// Stabile, begrenzte Korrelation für externe Adapter und Store-Subjects.
pub fn deterministic_invocation_id(kind: InvocationKind, parts: &[&str]) -> String {
    let hash = deterministic_hash(parts);
    let prefix = match kind {
        InvocationKind::Request => "request",
        InvocationKind::Reasoning => "reasoning",
        InvocationKind::Execution => "execution",
        InvocationKind::Output => "output",
    };
    format!("{prefix}:{hash:016x}")
}

/// Stabiler Fingerprint für den vollständigen, semantischen Invocation-Input.
pub fn deterministic_input_fingerprint(parts: &[&str]) -> String {
    format!("input:{:016x}", deterministic_hash(parts))
}

fn deterministic_hash(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in part.as_bytes().iter().chain([0xff].iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn new_state(spec: InvocationSpec, now: String) -> InvocationState {
    let artifact_id = ArtifactId(format!("invocation:{}", spec.invocation_id));
    InvocationState {
        invocation_id: spec.invocation_id,
        kind: spec.kind,
        capability: spec.capability,
        input_version: spec.input_version,
        input_fingerprint: spec.input_fingerprint,
        status: InvocationStatus::Prepared,
        result_version: None,
        error: None,
        retryable: false,
        created_at: now.clone(),
        updated_at: now,
        artifact_id,
        artifact_version: 1,
    }
}

fn assert_same_identity(
    state: &InvocationState,
    spec: &InvocationSpec,
) -> Result<(), InvocationError> {
    if state.kind != spec.kind
        || state.capability != spec.capability
        || state.input_version != spec.input_version
        || state.input_fingerprint != spec.input_fingerprint
    {
        return Err(InvocationError::IdentityConflict(
            spec.invocation_id.clone(),
        ));
    }
    Ok(())
}

fn invocation_subject(invocation_id: &str) -> String {
    format!("invocation:{invocation_id}")
}

fn state_artifact(
    state: &InvocationState,
    schemas: &SchemaRegistry,
) -> Result<Artifact, InvocationError> {
    let schema = schemas
        .get(&SchemaId(INVOCATION_SCHEMA_ID.into()))
        .ok_or_else(|| {
            InvocationError::InvalidState("invocation schema is not registered".into())
        })?;
    Ok(Artifact {
        artifact_id: state.artifact_id.clone(),
        version_id: VersionId(format!(
            "{}:v{}",
            state.artifact_id.0, state.artifact_version
        )),
        version: state.artifact_version,
        artifact_type: schema.artifact_type.clone(),
        schema_id: schema.id.clone(),
        schema_version: schema.version,
        created_at: state.updated_at.clone(),
        created_by: Actor {
            actor_type: ActorType::System,
            id: "arcs.runtime.invocation".into(),
        },
        source: Source {
            kind: SourceKind::Internal,
            reference: state.invocation_id.clone(),
        },
        trust: Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        stream_key: format!("invocation:{}", state.invocation_id),
        subject: Some(SubjectId(invocation_subject(&state.invocation_id))),
        tags: vec![format!("kind:{}", kind_name(state.kind))],
        payload: json!({
            "invocation_id": state.invocation_id,
            "kind": kind_name(state.kind),
            "capability": state.capability,
            "input_version": state.input_version.0,
            "input_fingerprint": state.input_fingerprint,
            "status": status_name(state.status),
            "result_version": state.result_version.as_ref().map_or("", |value| &value.0),
            "error": stored_error(state),
            "created_at": state.created_at,
            "updated_at": state.updated_at,
        }),
        provenance: Some(Provenance {
            parents: vec![state.input_version.0.clone()],
            rules_applied: vec!["runtime.invocation_state".into()],
            models_used: vec![],
            transform: Some("runtime.invocation".into()),
        }),
    })
}

fn parse_state(artifact: Artifact) -> Result<InvocationState, InvocationError> {
    #[derive(Deserialize)]
    struct Payload {
        invocation_id: String,
        kind: InvocationKind,
        capability: String,
        input_version: String,
        input_fingerprint: String,
        status: InvocationStatus,
        result_version: String,
        error: String,
        created_at: String,
        updated_at: String,
    }
    let payload: Payload = serde_json::from_value(artifact.payload)
        .map_err(|error| InvocationError::InvalidState(error.to_string()))?;
    let (error, retryable) = parse_error(&payload.error);
    Ok(InvocationState {
        invocation_id: payload.invocation_id,
        kind: payload.kind,
        capability: payload.capability,
        input_version: VersionId(payload.input_version),
        input_fingerprint: payload.input_fingerprint,
        status: payload.status,
        result_version: (!payload.result_version.is_empty())
            .then_some(VersionId(payload.result_version)),
        error,
        retryable,
        created_at: payload.created_at,
        updated_at: payload.updated_at,
        artifact_id: artifact.artifact_id,
        artifact_version: artifact.version,
    })
}

fn stored_error(state: &InvocationState) -> String {
    let Some(error) = &state.error else {
        return String::new();
    };
    let prefix = if state.retryable {
        RETRYABLE_ERROR_PREFIX
    } else {
        PERMANENT_ERROR_PREFIX
    };
    format!("{prefix}{error}")
}

fn parse_error(error: &str) -> (Option<String>, bool) {
    if error.is_empty() {
        (None, false)
    } else if let Some(error) = error.strip_prefix(RETRYABLE_ERROR_PREFIX) {
        (Some(error.into()), true)
    } else if let Some(error) = error.strip_prefix(PERMANENT_ERROR_PREFIX) {
        (Some(error.into()), false)
    } else {
        // V1-Stände vor der Retryability-Markierung bleiben aus
        // Sicherheitsgründen terminal und werden nie stillschweigend erneut
        // an einen externen Service gesendet.
        (Some(error.into()), false)
    }
}

fn is_allowed_transition(from: InvocationStatus, to: InvocationStatus, retryable: bool) -> bool {
    matches!(
        (from, to),
        (InvocationStatus::Prepared, InvocationStatus::Dispatched)
            | (
                InvocationStatus::Dispatched,
                InvocationStatus::Succeeded | InvocationStatus::Failed | InvocationStatus::Unknown
            )
            | (
                InvocationStatus::Unknown,
                InvocationStatus::Dispatched
                    | InvocationStatus::Succeeded
                    | InvocationStatus::Failed
            )
    ) || (from == InvocationStatus::Failed && retryable && to == InvocationStatus::Prepared)
}

fn ensure_transition(
    from: InvocationStatus,
    to: InvocationStatus,
    retryable: bool,
) -> Result<(), InvocationError> {
    if is_allowed_transition(from, to, retryable) {
        Ok(())
    } else {
        Err(InvocationError::InvalidTransition { from, to })
    }
}

fn kind_name(kind: InvocationKind) -> &'static str {
    match kind {
        InvocationKind::Request => "request",
        InvocationKind::Reasoning => "reasoning",
        InvocationKind::Execution => "execution",
        InvocationKind::Output => "output",
    }
}

fn status_name(status: InvocationStatus) -> &'static str {
    match status {
        InvocationStatus::Prepared => "prepared",
        InvocationStatus::Dispatched => "dispatched",
        InvocationStatus::Succeeded => "succeeded",
        InvocationStatus::Failed => "failed",
        InvocationStatus::Unknown => "unknown",
    }
}

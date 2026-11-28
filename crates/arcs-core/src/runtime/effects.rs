//! Dauerhaftes Betriebsbuch für externe Wirkungen.
//!
//! Eine externe Wirkung ist jede Handlung, die außerhalb der Instanz Spuren
//! hinterlässt. Das Buch hält für jede Wirkung den belegten Zustand fest:
//! geplant, abgesendet, bestätigt, ungeklärt oder fehlgeschlagen.
//!
//! Der tragende Zustand ist `ungeklärt`. Wurde eine Wirkung abgesendet, aber
//! ihr Ergebnis nicht bestätigt, darf sie nicht stillschweigend wiederholt
//! werden. Stattdessen verlangt der Core eine ausdrückliche Klärung, die
//! entscheidet, ob die Wirkung eingetreten ist oder nicht. Das Buch verspricht
//! damit keine Exactly-once-Semantik für fremde Systeme; es verhindert nur,
//! dass ARCS eine unkontrollierte Wiederholung auslöst.

use crate::core::security::Classification;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, Clock, Provenance, SchemaId, SchemaRegistry, Source,
    SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;

pub const SCHEMA: &str = "arcs.effect.v1";
pub const PUBLISHER: &str = "arcs.runtime.effects";

/// Zustand einer externen Wirkung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    /// Geplant, noch nicht abgesendet.
    Planned,
    /// Abgesendet; das Ergebnis ist noch nicht bestätigt.
    Dispatched,
    /// Erfolgreich bestätigt; die Wirkung ist abgeschlossen.
    Confirmed,
    /// Das Ergebnis ist ungeklärt; die Wirkung wird nicht wiederholt.
    Unclear,
    /// Die Wirkung ist fehlgeschlagen und hat nicht stattgefunden.
    Failed,
}

impl EffectState {
    /// Stabile Kurzbezeichnung für Berichte und Protokolle.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Dispatched => "dispatched",
            Self::Confirmed => "confirmed",
            Self::Unclear => "unclear",
            Self::Failed => "failed",
        }
    }
}

/// Der persistierte Eintrag einer externen Wirkung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRecord {
    /// Stabile Identität der Wirkung.
    pub operation_id: String,
    /// Logischer Schlüssel der Aufgabe, die die Wirkung verantwortet.
    pub task_key: String,
    /// Art der Wirkung, etwa `act:<adapter>`.
    pub kind: String,
    /// Zugehöriger Aufruf in der Invocation-Deduplizierung.
    pub invocation_id: String,
    /// Belegter Zustand der Wirkung.
    pub state: EffectState,
    /// Beleg der externen Wirkung, sobald sie bestätigt ist.
    pub effect_reference: Option<String>,
    /// Anzahl der Versuche.
    pub attempt: u32,
    /// Begründung oder Fehlertext.
    pub detail: Option<String>,
    /// Version und Version-ID des gespeicherten Artefakts.
    pub version: u64,
    pub version_id: VersionId,
}

/// Entwurf einer neuen externen Wirkung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDraft {
    pub operation_id: String,
    pub task_key: String,
    pub kind: String,
    pub invocation_id: String,
}

/// Ergebnis einer ausdrücklichen Klärung.
///
/// Die Klärung ist eine Entscheidung des Betreibers. Sie trägt deshalb immer
/// eine Begründung, damit die Entscheidung in der Historie des Eintrags
/// nachvollziehbar bleibt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectResolution {
    /// Die Wirkung ist eingetreten; der Verweis belegt sie.
    Occurred { reference: String, reason: String },
    /// Die Wirkung ist nicht eingetreten; ein erneuter Versuch ist zulässig.
    DidNotOccur { reason: String },
}

/// Entscheidung der Dispatch-Grenze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Die Wirkung hat nicht stattgefunden und darf jetzt abgesendet werden.
    Send,
    /// Die Wirkung wurde bereits abgesendet, ihr Ausgang ist aber unbekannt.
    /// Dieselbe, idempotente Sendung darf wiederholt werden: Der Vertrag der
    /// Schnittstelle verhindert eine zweite Wirkung.
    Replay { attempt: u32 },
    /// Die Wirkung ist bereits eingetreten; ihr Beleg fehlt noch. Die erneute
    /// Sendung holt nur den Beleg zurück und erzeugt keine zweite Wirkung.
    RecoverEvidence { reference: String },
    /// Der Ausgang lässt sich der Wirkung nicht zuordnen; eine Klärung ist
    /// erforderlich.
    NeedsReconciliation { detail: String },
}

/// Leitet die nächste Fassung eines Eintrags aus dem aktuellen Stand ab.
fn derive(
    record: &EffectRecord,
    state: EffectState,
    attempt: u32,
    effect_reference: Option<String>,
    detail: Option<String>,
) -> EffectRecord {
    EffectRecord {
        operation_id: record.operation_id.clone(),
        task_key: record.task_key.clone(),
        kind: record.kind.clone(),
        invocation_id: record.invocation_id.clone(),
        state,
        effect_reference,
        attempt,
        detail,
        version: record.version.saturating_add(1),
        version_id: VersionId(String::new()),
    }
}

/// Fehler der Wirkungsgrenze.
#[derive(Debug)]
pub enum EffectError {
    /// Der Store hat die Änderung nicht angenommen.
    Store(StoreError),
    /// Die Anfrage ist strukturell unzulässig.
    Invalid(String),
    /// Für die Operation existiert kein Eintrag.
    UnknownOperation(String),
    /// Der Zustandswechsel ist nicht zulässig.
    IllegalTransition { from: EffectState, to: EffectState },
    /// Die zugehörige Invocation ließ sich nicht freigeben.
    Invocation(crate::runtime::invocation::InvocationError),
}

impl From<StoreError> for EffectError {
    fn from(value: StoreError) -> Self {
        EffectError::Store(value)
    }
}

impl From<crate::runtime::invocation::InvocationError> for EffectError {
    fn from(value: crate::runtime::invocation::InvocationError) -> Self {
        EffectError::Invocation(value)
    }
}

fn invalid(message: impl Into<String>) -> EffectError {
    EffectError::Invalid(message.into())
}

/// Dienst für das dauerhafte Buch externer Wirkungen.
///
/// Der Dienst kennt keine Fachlogik. Er hält fest, was geschehen ist, und
/// verhindert, dass eine ungeklärte Wirkung erneut abgesendet wird.
pub struct EffectService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}

impl<'a> EffectService<'a> {
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

    /// Legt eine geplante Wirkung an. Der Aufruf ist idempotent.
    pub fn plan(&self, draft: &EffectDraft) -> Result<EffectRecord, EffectError> {
        check_text("operation_id", &draft.operation_id)?;
        check_text("task_key", &draft.task_key)?;
        check_text("kind", &draft.kind)?;
        check_text("invocation_id", &draft.invocation_id)?;
        if let Some(existing) = self.lookup(&draft.operation_id)? {
            if existing.task_key != draft.task_key
                || existing.kind != draft.kind
                || existing.invocation_id != draft.invocation_id
            {
                return Err(invalid(format!(
                    "operation '{}' is already bound to a different effect",
                    draft.operation_id
                )));
            }
            return Ok(existing);
        }
        let record = EffectRecord {
            operation_id: draft.operation_id.clone(),
            task_key: draft.task_key.clone(),
            kind: draft.kind.clone(),
            invocation_id: draft.invocation_id.clone(),
            state: EffectState::Planned,
            effect_reference: None,
            attempt: 0,
            detail: None,
            version: 1,
            version_id: VersionId(String::new()),
        };
        self.write(&record, None)
    }

    /// Liest den aktuellen Eintrag einer Wirkung.
    pub fn lookup(&self, operation_id: &str) -> Result<Option<EffectRecord>, EffectError> {
        let Some(artifact) = self
            .store
            .current(&SubjectId(subject(operation_id)), &SchemaId(SCHEMA.into()))?
        else {
            return Ok(None);
        };
        Ok(Some(parse(&artifact)?))
    }

    /// Liest die vollständige Historie einer Wirkung.
    pub fn history(&self, operation_id: &str) -> Result<Vec<EffectRecord>, EffectError> {
        let artifacts = self
            .store
            .history(&SubjectId(subject(operation_id)), &SchemaId(SCHEMA.into()))?;
        artifacts.iter().map(parse).collect()
    }

    /// Entscheidet, ob eine Wirkung abgesendet werden darf.
    ///
    /// Eine Wirkung, die nicht stattgefunden hat, darf gesendet werden. Eine
    /// bereits abgesendete Wirkung darf wiederholt werden, solange dieselbe
    /// idempotente Sendung verwendet wird: Der Vertrag der Schnittstelle
    /// verhindert, dass die Wiederholung eine zweite Wirkung erzeugt. Eine
    /// bestätigte Wirkung darf nur erneut gesendet werden, um ihren fehlenden
    /// Beleg zu holen. Lässt sich der Ausgang keiner Wirkung zuordnen,
    /// verlangt die Grenze zuerst eine ausdrückliche Klärung.
    pub fn authorize_dispatch(&self, operation_id: &str) -> Result<DispatchDecision, EffectError> {
        match self.lookup(operation_id)? {
            None => Ok(DispatchDecision::Send),
            Some(record) => match record.state {
                EffectState::Planned | EffectState::Failed => Ok(DispatchDecision::Send),
                EffectState::Dispatched => Ok(DispatchDecision::Replay {
                    attempt: record.attempt,
                }),
                EffectState::Confirmed => Ok(DispatchDecision::RecoverEvidence {
                    reference: record.effect_reference.unwrap_or_default(),
                }),
                EffectState::Unclear => Ok(DispatchDecision::NeedsReconciliation {
                    detail: record
                        .detail
                        .unwrap_or_else(|| "external effect outcome cannot be attributed".into()),
                }),
            },
        }
    }

    /// Zählt die Wiederholung derselben Wirkung.
    ///
    /// Der Zustand bleibt `dispatched`: Die Wirkung ist ausgegangen, ihr
    /// Ausgang ist aber unbekannt. Nur die Zahl der Versuche steigt, damit
    /// die Historie die Wiederholung sichtbar hält.
    pub fn mark_replayed(
        &self,
        operation_id: &str,
        detail: Option<String>,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if record.state != EffectState::Dispatched {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Dispatched,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Dispatched,
            record.attempt.saturating_add(1),
            record.effect_reference.clone(),
            detail,
        );
        self.write(&next, Some(&previous))
    }

    /// Zählt einen erneuten Sendeversuch, der nur den Beleg zurückholt.
    ///
    /// Der Zustand bleibt `confirmed`: Die Wirkung ist eingetreten und wird
    /// nicht wiederholt. Nur die Zahl der Versuche steigt.
    pub fn mark_evidence_recovery(&self, operation_id: &str) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if record.state != EffectState::Confirmed {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Confirmed,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Confirmed,
            record.attempt.saturating_add(1),
            record.effect_reference.clone(),
            Some("evidence recovery for a confirmed effect".into()),
        );
        self.write(&next, Some(&previous))
    }

    /// Markiert den Beginn eines externen Aufrufs.
    pub fn mark_dispatched(
        &self,
        operation_id: &str,
        detail: Option<String>,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if !matches!(record.state, EffectState::Planned | EffectState::Failed) {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Dispatched,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Dispatched,
            record.attempt.saturating_add(1),
            record.effect_reference.clone(),
            detail,
        );
        self.write(&next, Some(&previous))
    }

    /// Bestätigt eine extern belegte Wirkung.
    pub fn confirm(
        &self,
        operation_id: &str,
        reference: &str,
    ) -> Result<EffectRecord, EffectError> {
        check_reference(reference)?;
        let record = self.require(operation_id)?;
        if !matches!(
            record.state,
            EffectState::Dispatched | EffectState::Unclear | EffectState::Planned
        ) {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Confirmed,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Confirmed,
            record.attempt,
            Some(reference.to_string()),
            None,
        );
        self.write(&next, Some(&previous))
    }

    /// Markiert eine Wirkung als ungeklärt. Der Zustand ist nicht terminal:
    /// Er wartet auf eine ausdrückliche Klärung.
    pub fn mark_unclear(
        &self,
        operation_id: &str,
        detail: &str,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if record.state != EffectState::Dispatched {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Unclear,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Unclear,
            record.attempt,
            record.effect_reference.clone(),
            Some(detail.to_string()),
        );
        self.write(&next, Some(&previous))
    }

    /// Markiert eine Wirkung als fehlgeschlagen. Die Wirkung hat nicht
    /// stattgefunden und darf erneut versucht werden.
    pub fn mark_failed(
        &self,
        operation_id: &str,
        detail: &str,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if matches!(record.state, EffectState::Confirmed) {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Failed,
            });
        }
        let previous = record.version_id.clone();
        let next = derive(
            &record,
            EffectState::Failed,
            record.attempt,
            record.effect_reference.clone(),
            Some(detail.to_string()),
        );
        self.write(&next, Some(&previous))
    }

    /// Klärt eine ungeklärte Wirkung ausdrücklich.
    ///
    /// Nur diese Klärung löst den Wartezustand. Sie entscheidet, ob die
    /// Wirkung eingetreten ist oder nicht, und wird als eigene Version des
    /// Eintrags festgehalten.
    pub fn reconcile(
        &self,
        operation_id: &str,
        resolution: &EffectResolution,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.require(operation_id)?;
        if record.state != EffectState::Unclear {
            return Err(EffectError::IllegalTransition {
                from: record.state,
                to: EffectState::Unclear,
            });
        }
        let previous = record.version_id.clone();
        match resolution {
            EffectResolution::Occurred { reference, reason } => {
                check_reference(reference)?;
                check_text("reason", reason)?;
                let next = derive(
                    &record,
                    EffectState::Confirmed,
                    record.attempt,
                    Some(reference.clone()),
                    Some(reason.clone()),
                );
                self.write(&next, Some(&previous))
            }
            EffectResolution::DidNotOccur { reason } => {
                check_text("reason", reason)?;
                let next = derive(
                    &record,
                    EffectState::Failed,
                    record.attempt,
                    record.effect_reference.clone(),
                    Some(reason.clone()),
                );
                self.write(&next, Some(&previous))
            }
        }
    }

    /// Klärt eine ungeklärte Wirkung und gibt die zugehörige Invocation
    /// wieder frei.
    ///
    /// Erst die ausdrückliche Klärung entscheidet über den weiteren Weg. Wurde
    /// festgestellt, dass die Wirkung nicht eingetreten ist, darf dieselbe
    /// Aufgabe denselben Auftrag erneut versuchen. Die Identität der
    /// Invocation bleibt dabei unverändert.
    pub fn reconcile_and_release(
        &self,
        operation_id: &str,
        resolution: &EffectResolution,
    ) -> Result<EffectRecord, EffectError> {
        let record = self.reconcile(operation_id, resolution)?;
        if record.state != EffectState::Failed {
            return Ok(record);
        }
        let invocations = crate::runtime::invocation::InvocationService::new(
            self.store,
            self.schemas,
            self.clock,
        );
        let Some(state) = invocations.lookup(&record.invocation_id)? else {
            return Ok(record);
        };
        if state.status == crate::runtime::invocation::InvocationStatus::Failed {
            invocations.release_failed(&state)?;
        }
        Ok(record)
    }

    fn require(&self, operation_id: &str) -> Result<EffectRecord, EffectError> {
        self.lookup(operation_id)?
            .ok_or_else(|| EffectError::UnknownOperation(operation_id.to_string()))
    }

    fn write(
        &self,
        record: &EffectRecord,
        previous: Option<&VersionId>,
    ) -> Result<EffectRecord, EffectError> {
        let schema = self
            .schemas
            .get(&SchemaId(SCHEMA.into()))
            .ok_or_else(|| invalid("missing effect schema"))?;
        let id = artifact_id(&record.operation_id);
        let version = record.version;
        let artifact = Artifact {
            artifact_id: ArtifactId(id.clone()),
            version_id: VersionId(format!("{id}:v{version}")),
            version,
            artifact_type: schema.artifact_type.clone(),
            schema_id: schema.id.clone(),
            schema_version: schema.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: record.operation_id.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: crate::core::SourceClass::System,
            },
            stream_key: format!("effect:{}", record.operation_id),
            subject: Some(SubjectId(subject(&record.operation_id))),
            tags: vec![format!("state:{}", record.state.as_str())],
            payload: serde_json::to_value(payload(record)).map_err(|e| invalid(e.to_string()))?,
            provenance: Some(Provenance {
                parents: previous
                    .map(|version| vec![version.0.clone()])
                    .unwrap_or_default(),
                rules_applied: vec!["runtime.effect".into()],
                models_used: vec![],
                transform: Some("runtime.effect".into()),
            }),
            security: crate::core::security::SecurityProfile::declared(Classification::Internal),
        };
        let stored = self
            .store
            .append_current_resolved(&artifact, self.schemas)?;
        parse(&stored)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectPayload {
    operation_id: String,
    task_key: String,
    kind: String,
    invocation_id: String,
    state: EffectState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect_reference: Option<String>,
    attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn payload(record: &EffectRecord) -> EffectPayload {
    EffectPayload {
        operation_id: record.operation_id.clone(),
        task_key: record.task_key.clone(),
        kind: record.kind.clone(),
        invocation_id: record.invocation_id.clone(),
        state: record.state,
        effect_reference: record.effect_reference.clone(),
        attempt: record.attempt,
        detail: record.detail.clone(),
    }
}

fn parse(artifact: &Artifact) -> Result<EffectRecord, EffectError> {
    if artifact.schema_id.0 != SCHEMA
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PUBLISHER
        || artifact.source.kind != SourceKind::Internal
    {
        return Err(invalid("untrusted effect record"));
    }
    let payload: EffectPayload =
        serde_json::from_value(artifact.payload.clone()).map_err(|e| invalid(e.to_string()))?;
    let id = artifact_id(&payload.operation_id);
    if artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&SubjectId(subject(&payload.operation_id)))
    {
        return Err(invalid("effect record identity changed"));
    }
    Ok(EffectRecord {
        operation_id: payload.operation_id,
        task_key: payload.task_key,
        kind: payload.kind,
        invocation_id: payload.invocation_id,
        state: payload.state,
        effect_reference: payload.effect_reference,
        attempt: payload.attempt,
        detail: payload.detail,
        version: artifact.version,
        version_id: artifact.version_id.clone(),
    })
}

fn subject(operation_id: &str) -> String {
    format!("effect:{operation_id}")
}

fn artifact_id(operation_id: &str) -> String {
    format!("effect:{}", stable_hash(operation_id))
}

/// Stabile, nur vom Bezeichner abhängige Kennung.
fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes().iter().chain([0xff].iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn check_text(field: &str, value: &str) -> Result<(), EffectError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} is empty, too long, or not printable"
        )));
    }
    Ok(())
}

/// Prüft einen externen Beleg.
///
/// Ein Beleg stammt aus einem fremden System und folgt dessen Regeln. Er
/// darf deshalb länger sein als ein eigener Bezeichner, muss aber lesbar
/// bleiben.
fn check_reference(value: &str) -> Result<(), EffectError> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(invalid(
            "the effect reference is empty, too long, or not printable",
        ));
    }
    Ok(())
}

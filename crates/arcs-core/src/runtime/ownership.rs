//! Persistente Aufgaben-Leases mit Fencing-Token.
//!
//! Eine Lease ist die persistente Zugehörigkeit einer Aufgabe zu genau einem
//! Eigentümer. Sie verhindert, dass zwei Instanzen dieselbe Aufgabe gleichzeitig
//! bearbeiten, und sie überlebt einen Neustart, weil sie als Core-Artefakt
//! gespeichert wird.
//!
//! Jede neue Zuteilung erhöht das Fencing-Token der Aufgabe. Ein zurückgebliebener
//! Eigentümer, dessen Lease abgelaufen oder übernommen wurde, kann deshalb nicht
//! mehr schreiben: Sein Token ist nicht mehr das aktuelle. Das Token ersetzt
//! keine fachliche Prüfung, es macht aber den Besitzwechsel eindeutig.

use crate::core::artifact_factory::{format_rfc3339, unix_seconds_from_rfc3339};
use crate::core::security::Classification;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, Clock, Provenance, SchemaId, SchemaRegistry, Source,
    SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "ownership_tests.rs"]
mod tests;

pub const SCHEMA: &str = "arcs.task_lease.v1";
pub const PUBLISHER: &str = "arcs.runtime.ownership";

/// Obergrenze der Lease-Dauer. Eine Lease ist eine Arbeitszusage, kein
/// dauerhafter Besitz.
pub const MAX_LEASE_SECONDS: u64 = 86_400;

/// Zustand einer Aufgaben-Lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    /// Die Lease ist wirksam.
    Active,
    /// Die Lease wurde ordnungsgemäß abgegeben.
    Released,
}

impl LeaseState {
    /// Stabile Kurzbezeichnung für Berichte und Protokolle.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Released => "released",
        }
    }
}

/// Der persistierte Eintrag einer Aufgaben-Lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLease {
    /// Stabile Identität der Zuteilung.
    pub lease_id: String,
    /// Logischer Schlüssel der Aufgabe.
    pub task_key: String,
    /// Eigentümer, dem die Aufgabe zugehört.
    pub owner_id: String,
    /// Monoton steigendes Token der Zuteilung.
    pub fencing_token: u64,
    /// Zeitpunkt der Zuteilung.
    pub acquired_at: String,
    /// Zeitpunkt, zu dem die Zuteilung ohne Erneuerung erlischt.
    pub expires_at: String,
    /// Zustand der Zuteilung.
    pub state: LeaseState,
    /// Version und Version-ID des gespeicherten Artefakts.
    pub version: u64,
    pub version_id: VersionId,
}

/// Fehler der Eigentumsgrenze.
#[derive(Debug)]
pub enum OwnershipError {
    /// Der Store hat die Änderung nicht angenommen.
    Store(StoreError),
    /// Die Anfrage ist strukturell unzulässig.
    Invalid(String),
    /// Die Aufgabe ist bereits einem anderen Eigentümer zugewiesen.
    LeaseHeld {
        owner_id: String,
        expires_at: String,
    },
    /// Der Eigentümer passt nicht zur Zuteilung.
    NotOwner { expected: String, actual: String },
    /// Das vorgelegte Fencing-Token ist nicht mehr das aktuelle.
    FencingTokenMismatch { expected: u64, presented: u64 },
    /// Die Zuteilung ist abgelaufen.
    LeaseExpired { expires_at: String },
}

impl From<StoreError> for OwnershipError {
    fn from(value: StoreError) -> Self {
        OwnershipError::Store(value)
    }
}

fn invalid(message: impl Into<String>) -> OwnershipError {
    OwnershipError::Invalid(message.into())
}

/// Dienst für die persistenten Aufgaben-Leases.
///
/// Der Dienst kennt keine Fachlogik. Er verwaltet ausschließlich Zugehörigkeit,
/// Ablauf und das Fencing-Token einer Aufgabe.
pub struct OwnershipService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}

impl<'a> OwnershipService<'a> {
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

    /// Liest die aktuelle Lease einer Aufgabe.
    pub fn current(&self, task_key: &str) -> Result<Option<TaskLease>, OwnershipError> {
        let Some(artifact) = self
            .store
            .current(&SubjectId(subject(task_key)), &SchemaId(SCHEMA.into()))?
        else {
            return Ok(None);
        };
        Ok(Some(parse(&artifact)?))
    }

    /// Lädt die vollständige Historie der Leases einer Aufgabe.
    pub fn history(&self, task_key: &str) -> Result<Vec<TaskLease>, OwnershipError> {
        let artifacts = self
            .store
            .history(&SubjectId(subject(task_key)), &SchemaId(SCHEMA.into()))?;
        artifacts.iter().map(parse).collect()
    }

    /// Zuteilt die Aufgabe einem Eigentümer.
    ///
    /// Der Aufruf ist idempotent: Ein gültiger Eigentümer erhält seine
    /// bestehende Zuteilung unverändert zurück. Ein fremder Eigentümer erhält
    /// die Ablehnung, solange die Zuteilung wirksam ist. Nach Ablauf oder
    /// Abgabe wird die Aufgabe neu zugeteilt; das Fencing-Token steigt dabei.
    pub fn acquire(
        &self,
        task_key: &str,
        owner_id: &str,
        lease_seconds: u64,
    ) -> Result<TaskLease, OwnershipError> {
        check_text("task_key", task_key)?;
        check_text("owner_id", owner_id)?;
        if lease_seconds == 0 || lease_seconds > MAX_LEASE_SECONDS {
            return Err(invalid("lease_seconds is out of range"));
        }
        let now = self.clock.now_rfc3339();
        let current = self.current(task_key)?;
        if let Some(lease) = &current
            && lease.state == LeaseState::Active
            && !is_expired(&lease.expires_at, &now)
        {
            if lease.owner_id == owner_id {
                return Ok(lease.clone());
            }
            return Err(OwnershipError::LeaseHeld {
                owner_id: lease.owner_id.clone(),
                expires_at: lease.expires_at.clone(),
            });
        }
        let token = current
            .as_ref()
            .map_or(1, |lease| lease.fencing_token.saturating_add(1));
        let version = current
            .as_ref()
            .map_or(1, |lease| lease.version.saturating_add(1));
        let lease = TaskLease {
            lease_id: lease_id(task_key, token),
            task_key: task_key.to_string(),
            owner_id: owner_id.to_string(),
            fencing_token: token,
            acquired_at: now.clone(),
            expires_at: self.expires_at(&now, lease_seconds)?,
            state: LeaseState::Active,
            version,
            version_id: VersionId(String::new()),
        };
        match self.write(&lease, current.as_ref().map(|lease| &lease.version_id)) {
            Ok(stored) => Ok(stored),
            Err(OwnershipError::Store(StoreError::VersionConflict { .. })) => {
                // Ein konkurrierender Eigentümer hat zuerst zugeslagen. Der
                // Verlierer darf die Aufgabe nicht trotzdem beanspruchen.
                match self.current(task_key)? {
                    Some(lease)
                        if lease.state == LeaseState::Active
                            && lease.owner_id != owner_id
                            && !is_expired(&lease.expires_at, &now) =>
                    {
                        Err(OwnershipError::LeaseHeld {
                            owner_id: lease.owner_id.clone(),
                            expires_at: lease.expires_at.clone(),
                        })
                    }
                    _ => Err(OwnershipError::Store(StoreError::VersionConflict {
                        expected: lease.version,
                        actual: lease.version,
                    })),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Verlängert eine wirksame Zuteilung, ohne das Token zu verändern.
    pub fn renew(
        &self,
        lease: &TaskLease,
        owner_id: &str,
        lease_seconds: u64,
    ) -> Result<TaskLease, OwnershipError> {
        if lease_seconds == 0 || lease_seconds > MAX_LEASE_SECONDS {
            return Err(invalid("lease_seconds is out of range"));
        }
        let now = self.clock.now_rfc3339();
        let current = self.current(&lease.task_key)?;
        let Some(current) = current else {
            return Err(OwnershipError::NotOwner {
                expected: lease.owner_id.clone(),
                actual: owner_id.to_string(),
            });
        };
        if current.fencing_token != lease.fencing_token {
            return Err(OwnershipError::FencingTokenMismatch {
                expected: current.fencing_token,
                presented: lease.fencing_token,
            });
        }
        if current.owner_id != owner_id || lease.owner_id != owner_id {
            return Err(OwnershipError::NotOwner {
                expected: current.owner_id.clone(),
                actual: owner_id.to_string(),
            });
        }
        if current.state != LeaseState::Active {
            return Err(OwnershipError::LeaseExpired {
                expires_at: current.expires_at.clone(),
            });
        }
        if is_expired(&current.expires_at, &now) {
            return Err(OwnershipError::LeaseExpired {
                expires_at: current.expires_at.clone(),
            });
        }
        let previous = current.version_id.clone();
        let renewed = TaskLease {
            expires_at: self.expires_at(&now, lease_seconds)?,
            version: current.version.saturating_add(1),
            ..current
        };
        self.write(&renewed, Some(&previous))
    }

    /// Prüft, ob eine Zuteilung weiterhin wirksam ist.
    ///
    /// Die Prüfung schlägt fehl, sobald die Lease abgelaufen ist, ein anderer
    /// Eigentümer übernommen hat oder das Token nicht mehr das aktuelle ist.
    pub fn validate(&self, lease: &TaskLease) -> Result<(), OwnershipError> {
        let now = self.clock.now_rfc3339();
        let Some(current) = self.current(&lease.task_key)? else {
            return Err(OwnershipError::FencingTokenMismatch {
                expected: lease.fencing_token,
                presented: lease.fencing_token,
            });
        };
        if current.fencing_token != lease.fencing_token {
            return Err(OwnershipError::FencingTokenMismatch {
                expected: current.fencing_token,
                presented: lease.fencing_token,
            });
        }
        if current.owner_id != lease.owner_id {
            return Err(OwnershipError::NotOwner {
                expected: current.owner_id.clone(),
                actual: lease.owner_id.clone(),
            });
        }
        if current.state != LeaseState::Active {
            return Err(OwnershipError::LeaseExpired {
                expires_at: current.expires_at.clone(),
            });
        }
        if is_expired(&current.expires_at, &now) {
            return Err(OwnershipError::LeaseExpired {
                expires_at: current.expires_at.clone(),
            });
        }
        Ok(())
    }

    /// Gibt eine Zuteilung ab. Ein fremder Eigentümer kann sie nicht abgeben.
    pub fn release(&self, lease: &TaskLease) -> Result<TaskLease, OwnershipError> {
        let current = self.current(&lease.task_key)?;
        let Some(current) = current else {
            return Ok(lease.clone());
        };
        if current.fencing_token != lease.fencing_token {
            return Err(OwnershipError::FencingTokenMismatch {
                expected: current.fencing_token,
                presented: lease.fencing_token,
            });
        }
        if current.owner_id != lease.owner_id {
            return Err(OwnershipError::NotOwner {
                expected: current.owner_id.clone(),
                actual: lease.owner_id.clone(),
            });
        }
        if current.state == LeaseState::Released {
            return Ok(current);
        }
        let previous = current.version_id.clone();
        let released = TaskLease {
            state: LeaseState::Released,
            version: current.version.saturating_add(1),
            ..current
        };
        self.write(&released, Some(&previous))
    }

    fn expires_at(&self, now: &str, lease_seconds: u64) -> Result<String, OwnershipError> {
        let seconds = unix_seconds_from_rfc3339(now)
            .ok_or_else(|| invalid("clock returned an unusable timestamp"))?;
        let target = seconds
            .checked_add(lease_seconds)
            .ok_or_else(|| invalid("lease expiry overflows"))?;
        Ok(format_rfc3339(target))
    }

    fn write(
        &self,
        lease: &TaskLease,
        previous: Option<&VersionId>,
    ) -> Result<TaskLease, OwnershipError> {
        let schema = self
            .schemas
            .get(&SchemaId(SCHEMA.into()))
            .ok_or_else(|| invalid("missing task lease schema"))?;
        let id = artifact_id(&lease.task_key);
        let version = lease.version;
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
                reference: lease.lease_id.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: crate::core::SourceClass::System,
            },
            stream_key: format!("lease:{}", lease.task_key),
            subject: Some(SubjectId(subject(&lease.task_key))),
            tags: vec![
                format!("state:{}", lease.state.as_str()),
                format!("owner:{}", lease.owner_id),
            ],
            payload: serde_json::to_value(payload(lease)).map_err(|e| invalid(e.to_string()))?,
            provenance: Some(Provenance {
                parents: previous
                    .map(|version| vec![version.0.clone()])
                    .unwrap_or_default(),
                rules_applied: vec!["runtime.task_lease".into()],
                models_used: vec![],
                transform: Some("runtime.task_lease".into()),
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
struct LeasePayload {
    lease_id: String,
    task_key: String,
    owner_id: String,
    fencing_token: u64,
    acquired_at: String,
    expires_at: String,
    state: LeaseState,
}

fn payload(lease: &TaskLease) -> LeasePayload {
    LeasePayload {
        lease_id: lease.lease_id.clone(),
        task_key: lease.task_key.clone(),
        owner_id: lease.owner_id.clone(),
        fencing_token: lease.fencing_token,
        acquired_at: lease.acquired_at.clone(),
        expires_at: lease.expires_at.clone(),
        state: lease.state,
    }
}

fn parse(artifact: &Artifact) -> Result<TaskLease, OwnershipError> {
    if artifact.schema_id.0 != SCHEMA
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PUBLISHER
        || artifact.source.kind != SourceKind::Internal
    {
        return Err(invalid("untrusted task lease"));
    }
    let payload: LeasePayload =
        serde_json::from_value(artifact.payload.clone()).map_err(|e| invalid(e.to_string()))?;
    let id = artifact_id(&payload.task_key);
    if artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&SubjectId(subject(&payload.task_key)))
    {
        return Err(invalid("task lease identity changed"));
    }
    Ok(TaskLease {
        lease_id: payload.lease_id,
        task_key: payload.task_key,
        owner_id: payload.owner_id,
        fencing_token: payload.fencing_token,
        acquired_at: payload.acquired_at,
        expires_at: payload.expires_at,
        state: payload.state,
        version: artifact.version,
        version_id: artifact.version_id.clone(),
    })
}

fn subject(task_key: &str) -> String {
    format!("lease:{task_key}")
}

fn artifact_id(task_key: &str) -> String {
    format!("lease:{:016x}", stable_hash(task_key))
}

fn lease_id(task_key: &str, token: u64) -> String {
    format!("lease:{:016x}:{token}", stable_hash(task_key))
}

/// Stabile, nur vom Aufgabenschlüssel abhängige Kennung.
fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes().iter().chain([0xff].iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Prüft, ob eine Zuteilung zum Zeitpunkt `now` abgelaufen ist.
fn is_expired(expires_at: &str, now: &str) -> bool {
    match (
        unix_seconds_from_rfc3339(expires_at),
        unix_seconds_from_rfc3339(now),
    ) {
        (Some(expires), Some(now)) => now >= expires,
        // Ein unlesbarer Zeitstempel gilt als abgelaufen: fail-closed.
        _ => true,
    }
}

fn check_text(field: &str, value: &str) -> Result<(), OwnershipError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} is empty, too long, or not printable"
        )));
    }
    Ok(())
}

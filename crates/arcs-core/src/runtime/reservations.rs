//! Persistent cooperative ownership. No expiry or controller fencing is implied.
mod records;
#[cfg(test)]
mod tests;

use crate::core::{Actor, ActorType, Artifact, Clock, SchemaRegistry, VersionId};
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "arcs.resource_reservation.v1";
pub const PUBLISHER: &str = "arcs.runtime.reservations";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationOwner {
    pub definition_version: VersionId,
    pub input_version: VersionId,
    pub task_sequence: u64,
    pub session: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationState {
    Held,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub resource: String,
    pub owner: ReservationOwner,
    pub version_id: VersionId,
    pub state: ReservationState,
    pub decision_by: Actor,
    pub reason: String,
    pub evidence: Vec<VersionId>,
}
#[derive(Debug)]
pub enum ReservationError {
    Store(StoreError),
    Invalid(String),
    Busy {
        resource: String,
        owner: ReservationOwner,
    },
    Conflict,
}
impl From<StoreError> for ReservationError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
fn invalid(message: impl Into<String>) -> ReservationError {
    ReservationError::Invalid(message.into())
}

pub struct ReservationService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}
impl<'a> ReservationService<'a> {
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

    pub fn current(&self, resource: &str) -> Result<Option<Reservation>, ReservationError> {
        self.load(resource)?.map(|a| records::parse(&a)).transpose()
    }

    pub fn acquire(
        &self,
        owner: &ReservationOwner,
        resources: &[String],
    ) -> Result<Vec<Reservation>, ReservationError> {
        self.validate_owner(owner)?;
        let resources = validate_resources(resources)?;
        for _ in 0..3 {
            let mut artifacts = Vec::new();
            let mut expected = Vec::new();
            let mut result = Vec::new();
            for resource in &resources {
                let previous = self.load(resource)?;
                expected.push((
                    records::subject(resource),
                    previous.as_ref().map(|a| a.version_id.clone()),
                ));
                if let Some(record) = previous.as_ref().map(records::parse).transpose()? {
                    if record.owner == *owner {
                        if record.state == ReservationState::Released {
                            return Err(invalid(
                                "a released control session cannot reacquire resources",
                            ));
                        }
                        result.push(record);
                        continue;
                    }
                    if record.state == ReservationState::Held {
                        return Err(ReservationError::Busy {
                            resource: resource.clone(),
                            owner: record.owner,
                        });
                    }
                }
                let artifact = records::make(
                    resource,
                    owner,
                    ReservationState::Held,
                    previous.as_ref(),
                    &Actor {
                        actor_type: ActorType::System,
                        id: PUBLISHER.into(),
                    },
                    "goal control session",
                    &[],
                    self.clock,
                    self.schemas,
                )?;
                result.push(records::parse(&artifact)?);
                artifacts.push(artifact);
            }
            if artifacts.is_empty() {
                return Ok(result);
            }
            match self
                .store
                .append_reservations(&artifacts, self.schemas, &expected)
            {
                Ok(()) => return Ok(result),
                Err(StoreError::VersionConflict { .. } | StoreError::IdempotencyConflict) => {
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(ReservationError::Conflict)
    }

    /// Exact held revisions are mandatory. The caller authenticates the actor
    /// and decides whether its goal is closed or reconciled before invoking this.
    pub fn release(
        &self,
        owner: &ReservationOwner,
        expected: &[VersionId],
        actor: &Actor,
        reason: &str,
        evidence: &[VersionId],
    ) -> Result<(), ReservationError> {
        self.validate_owner(owner)?;
        if expected.is_empty()
            || expected.len() > 16
            || expected
                .iter()
                .map(|id| &id.0)
                .collect::<BTreeSet<_>>()
                .len()
                != expected.len()
            || !matches!(actor.actor_type, ActorType::Human | ActorType::System)
            || actor.id.trim().is_empty()
            || actor.id.len() > 128
            || reason.trim().is_empty()
            || reason.len() > 2000
            || evidence.len() > 32
        {
            return Err(invalid("invalid reservation release request"));
        }
        for id in evidence {
            if self.store.get(id)?.is_none() {
                return Err(invalid("missing reconciliation evidence"));
            }
        }
        let mut artifacts = Vec::new();
        let mut guards = Vec::new();
        for version in expected {
            let previous = self
                .store
                .get(version)?
                .ok_or_else(|| invalid("missing reservation revision"))?;
            let record = records::parse(&previous)?;
            if record.owner != *owner || record.state != ReservationState::Held {
                return Err(invalid("release does not match held owner"));
            }
            let current = self
                .load(&record.resource)?
                .ok_or_else(|| invalid("missing current reservation"))?;
            if current.version_id != *version {
                let released = records::parse(&current)?;
                if released.state == ReservationState::Released
                    && released.owner == *owner
                    && released.decision_by == *actor
                    && released.reason == reason
                    && released.evidence == evidence
                    && current
                        .provenance
                        .as_ref()
                        .is_some_and(|p| p.parents.contains(&version.0))
                {
                    continue;
                }
                return Err(ReservationError::Conflict);
            }
            artifacts.push(records::make(
                &record.resource,
                owner,
                ReservationState::Released,
                Some(&previous),
                actor,
                reason,
                evidence,
                self.clock,
                self.schemas,
            )?);
            guards.push((records::subject(&record.resource), Some(version.clone())));
        }
        if artifacts.is_empty() {
            return Ok(());
        }
        self.store
            .append_reservations(&artifacts, self.schemas, &guards)
            .map_err(|e| match e {
                StoreError::VersionConflict { .. } | StoreError::IdempotencyConflict => {
                    ReservationError::Conflict
                }
                e => e.into(),
            })
    }

    fn validate_owner(&self, owner: &ReservationOwner) -> Result<(), ReservationError> {
        if owner.task_sequence == 0 || owner.task_sequence > i64::MAX as u64 || owner.session == 0 {
            return Err(invalid("invalid control owner"));
        }
        let definition = self
            .store
            .get(&owner.definition_version)?
            .ok_or_else(|| invalid("missing runtime definition"))?;
        if definition.schema_id.0 != "arcs.input.host_runtime.v1"
            || definition.source.kind != crate::core::SourceKind::Internal
            || definition.created_by.actor_type != ActorType::System
            || definition.created_by.id != "arcs.host.runtime"
        {
            return Err(invalid(
                "control owner requires a trusted runtime definition",
            ));
        }
        if self.store.get(&owner.input_version)?.is_none() {
            return Err(invalid("missing owner input"));
        }
        Ok(())
    }
    fn load(&self, resource: &str) -> Result<Option<Artifact>, ReservationError> {
        let value = self.store.current(
            &records::subject(resource),
            &crate::core::SchemaId(SCHEMA.into()),
        )?;
        if let Some(a) = &value {
            let record = records::parse(a)?;
            if record.resource != resource {
                return Err(invalid("reservation resource identity changed"));
            }
        }
        Ok(value)
    }
}
pub fn validate_resources(resources: &[String]) -> Result<Vec<String>, ReservationError> {
    if resources.is_empty()
        || resources.len() > 16
        || resources.iter().any(|name| {
            name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        || resources.iter().collect::<BTreeSet<_>>().len() != resources.len()
    {
        return Err(invalid(
            "exclusive resources need 1..16 distinct bounded identifiers",
        ));
    }
    Ok(resources
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

//! Persistent ownership and retirement of dispatched reservation fences.
mod records;
#[cfg(test)]
mod tests;

use crate::adapters::{ActAdapter, ActFence, AdapterCallError, CapabilityRef};
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

pub struct ReservationRelease<'a> {
    pub owner: &'a ReservationOwner,
    pub expected: &'a [VersionId],
    pub actor: &'a Actor,
    pub reason: &'a str,
    pub evidence: &'a [VersionId],
}

/// An exact controller request issued by a validated release preparation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ControllerClosure {
    capability: CapabilityRef,
    fence: ActFence,
}

pub struct ControllerAcknowledgement(ControllerClosure);

impl ControllerClosure {
    pub fn capability(&self) -> &CapabilityRef {
        &self.capability
    }

    pub fn confirm(
        self,
        endpoint: &dyn ActAdapter,
    ) -> Result<ControllerAcknowledgement, AdapterCallError> {
        let manifest = endpoint.manifest();
        if manifest.adapter_id != self.capability.adapter_id
            || !manifest
                .capability(&self.capability.capability_id)
                .and_then(|cap| cap.contract.act())
                .is_some_and(|act| act.fenced_resource == Some(self.fence.resource.as_str()))
        {
            return Err(AdapterCallError::Rejected(
                "controller fence contract changed".into(),
            ));
        }
        endpoint.close_fence(&self.capability, &self.fence)?;
        Ok(ControllerAcknowledgement(self))
    }
}

pub struct PreparedReservationRelease<'a> {
    service: ReservationService<'a>,
    artifacts: Vec<Artifact>,
    guards: Vec<(crate::core::SubjectId, Option<VersionId>)>,
    controllers: Vec<ControllerClosure>,
}

impl PreparedReservationRelease<'_> {
    pub fn controllers(&self) -> &[ControllerClosure] {
        &self.controllers
    }

    pub fn commit_with_controllers<'endpoint>(
        self,
        mut endpoint: impl FnMut(&CapabilityRef) -> Result<&'endpoint dyn ActAdapter, ReservationError>,
    ) -> Result<(), ReservationError> {
        for closure in self.controllers() {
            let capability = closure.capability();
            let acknowledgement =
                closure
                    .clone()
                    .confirm(endpoint(capability)?)
                    .map_err(|source| ReservationError::ControllerAcknowledgement {
                        capability: capability.clone(),
                        source,
                    })?;
            self.service
                .record_controller_acknowledgement(acknowledgement)?;
        }
        self.commit()
    }

    /// Recheck the reservation head and all outstanding controllers atomically.
    pub fn commit(self) -> Result<(), ReservationError> {
        if self.artifacts.is_empty() {
            return Ok(());
        }
        self.service
            .commit(&self.artifacts, &self.guards)
            .map_err(|error| match error {
                StoreError::VersionConflict { .. } | StoreError::IdempotencyConflict => {
                    ReservationError::Conflict
                }
                StoreError::OpenActFences => ReservationError::ControllersPending,
                error => error.into(),
            })
    }
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
    ControllersPending,
    ControllerAcknowledgement {
        capability: CapabilityRef,
        source: AdapterCallError,
    },
}
impl From<StoreError> for ReservationError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
fn invalid(message: impl Into<String>) -> ReservationError {
    ReservationError::Invalid(message.into())
}

#[derive(Clone, Copy)]
pub struct ReservationService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}
impl<'a> ReservationService<'a> {
    fn commit(
        &self,
        artifacts: &[Artifact],
        expected: &[(crate::core::SubjectId, Option<VersionId>)],
    ) -> Result<(), StoreError> {
        self.store.append_reservations_compared(
            artifacts,
            self.schemas,
            &expected
                .iter()
                .map(|(subject, version)| {
                    (
                        subject.clone(),
                        crate::core::SchemaId(SCHEMA.into()),
                        version.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

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

    pub(crate) fn act_fence(&self, expected: &Reservation) -> Result<ActFence, ReservationError> {
        let artifact = self
            .load(&expected.resource)?
            .ok_or(ReservationError::Conflict)?;
        let current = records::parse(&artifact)?;
        if current.version_id != expected.version_id
            || current.owner != expected.owner
            || current.state != ReservationState::Held
            || expected.state != ReservationState::Held
        {
            return Err(ReservationError::Conflict);
        }
        let fence = ActFence {
            authority: self.store.authority_id()?,
            resource: current.resource,
            revision: artifact.version,
            reservation_version_id: current.version_id,
        };
        if !fence.is_valid() {
            return Err(invalid("invalid current reservation fence"));
        }
        Ok(fence)
    }

    fn register_controller(
        &self,
        fence: &ActFence,
        controller: &CapabilityRef,
    ) -> Result<bool, ReservationError> {
        self.store
            .register_act_controller(
                &records::subject(&fence.resource),
                &crate::core::SchemaId(SCHEMA.into()),
                &fence.reservation_version_id,
                controller,
            )
            .map_err(|error| match error {
                StoreError::IdempotencyConflict => ReservationError::Conflict,
                error => error.into(),
            })
    }

    // Register in the same Current-State guard used by release. This transaction
    // finishes before any adapter I/O.
    pub(crate) fn register_fenced_dispatch(
        &self,
        fence: &ActFence,
        controller: &CapabilityRef,
    ) -> Result<(), ReservationError> {
        if !self.register_controller(fence, controller)? {
            return Err(invalid(
                "reservation fence is already closed for this controller",
            ));
        }
        Ok(())
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
            match self.commit(&artifacts, &expected) {
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
        self.release_with_controllers(
            ReservationRelease {
                owner,
                expected,
                actor,
                reason,
                evidence,
            },
            &[],
            |_| Err(ReservationError::ControllersPending),
        )
    }

    /// Confirm every controller before the guarded Core release. Configured
    /// controllers also cover reservations dispatched before the guard existed.
    pub fn release_with_controllers<'endpoint>(
        &self,
        request: ReservationRelease<'_>,
        configured: &[(String, CapabilityRef)],
        endpoint: impl FnMut(&CapabilityRef) -> Result<&'endpoint dyn ActAdapter, ReservationError>,
    ) -> Result<(), ReservationError> {
        self.prepare_release(request, configured)?
            .commit_with_controllers(endpoint)
    }

    /// Record an actual controller reply on the Core thread after adapter I/O.
    pub fn record_controller_acknowledgement(
        &self,
        acknowledgement: ControllerAcknowledgement,
    ) -> Result<(), ReservationError> {
        let ControllerAcknowledgement(closure) = acknowledgement;
        if self.store.authority_id()? != closure.fence.authority {
            return Err(invalid(
                "controller acknowledgement belongs to another Core",
            ));
        }
        self.store.confirm_act_controller_closed(
            &closure.fence.reservation_version_id,
            &closure.capability,
        )?;
        Ok(())
    }

    pub fn prepare_release(
        &self,
        request: ReservationRelease<'_>,
        configured: &[(String, CapabilityRef)],
    ) -> Result<PreparedReservationRelease<'a>, ReservationError> {
        let ReservationRelease {
            owner,
            expected,
            actor,
            reason,
            evidence,
        } = request;
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
        let mut held = Vec::new();
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
            held.push(record);
        }
        let mut controllers = Vec::new();
        for reservation in held {
            let legacy = !configured.is_empty()
                && self
                    .store
                    .needs_legacy_act_controllers(&reservation.version_id)?;
            let mut declared = configured
                .iter()
                .filter(|(resource, _)| legacy && *resource == reservation.resource)
                .map(|(_, capability)| capability)
                .peekable();
            let mut pending: BTreeSet<_> = self
                .store
                .pending_act_controllers(&reservation.version_id)?
                .into_iter()
                .collect();
            if pending.is_empty() && declared.peek().is_none() {
                continue;
            }
            let fence = self.act_fence(&reservation)?;
            for controller in declared {
                if self.register_controller(&fence, controller)? {
                    pending.insert(controller.clone());
                } else {
                    pending.remove(controller);
                }
            }
            for controller in pending {
                controllers.push(ControllerClosure {
                    capability: controller,
                    fence: fence.clone(),
                });
            }
        }
        Ok(PreparedReservationRelease {
            service: *self,
            artifacts,
            guards,
            controllers,
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
        || resources.iter().any(|name| !ActFence::valid_resource(name))
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

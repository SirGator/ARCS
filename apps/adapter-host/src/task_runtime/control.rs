//! Cooperative exclusive goal rounds in the existing Core, without expiry.
mod review;
pub use review::ResourceEvidenceReview;

use super::{
    TaskRuntime,
    config::invalid,
    goal::{GoalConfig, GoalState},
    journal::ControlSession,
};
use crate::{Host, HostError};
use arcs::core::{Actor, ActorType, SystemClock, VersionId};
use arcs::runtime::reservations::{
    Reservation, ReservationError, ReservationService, ReservationState,
};
use std::collections::{BTreeSet, HashSet};

fn error(error: ReservationError) -> HostError {
    match error {
        ReservationError::Store(e) => HostError::Store(e),
        ReservationError::Busy { resource, .. } => HostError::ResourceBusy {
            resource,
            requested: 1,
            available: 0,
        },
        ReservationError::Conflict => HostError::ResourceBusy {
            resource: "exclusive-control-revision".into(),
            requested: 1,
            available: 0,
        },
        error => invalid(format!("exclusive control: {error:?}")),
    }
}

fn held_reservations(
    service: &ReservationService<'_>,
    session: &ControlSession,
) -> Result<Vec<Reservation>, HostError> {
    let mut held = Vec::new();
    for resource in &session.resources {
        if let Some(record) = service.current(resource).map_err(error)?
            && record.owner == session.owner
            && record.state == ReservationState::Held
        {
            held.push(record);
        }
    }
    Ok(held)
}

impl TaskRuntime {
    pub(super) fn verify_goal_control(
        &self,
        host: &Host,
        sequence: u64,
        config: &GoalConfig,
    ) -> Result<(), HostError> {
        if config.exclusive_resources.is_empty() {
            return Ok(());
        }
        let session = self
            .journal
            .control_session(sequence)?
            .ok_or_else(|| invalid("missing control session"))?;
        if session.closed {
            return Err(invalid("control session already closed"));
        }
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        for resource in &session.resources {
            if !service
                .current(resource)
                .map_err(error)?
                .is_some_and(|r| r.state == ReservationState::Held && r.owner == session.owner)
            {
                return Err(invalid("goal no longer owns its exclusive resource"));
            }
        }
        Ok(())
    }
    pub(super) fn acquire_control(
        &self,
        host: &Host,
        sequence: u64,
        config: &GoalConfig,
        input: &VersionId,
    ) -> Result<(), HostError> {
        if config.exclusive_resources.is_empty() {
            return Ok(());
        }
        if let Some(goal) = self.saved_goal(sequence)?
            && (goal.state.terminal()
                || matches!(goal.state, GoalState::WaitingTime | GoalState::WaitingEvent))
        {
            return self.cleanup_control(host, sequence);
        }
        let resources =
            arcs::runtime::reservations::validate_resources(&config.exclusive_resources)
                .map_err(error)?;
        let session = self
            .journal
            .ensure_control_session(sequence, input, &resources)?;
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let already_owned = resources
            .iter()
            .map(|r| service.current(r))
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?
            .iter()
            .all(|r| {
                r.as_ref()
                    .is_some_and(|r| r.state == ReservationState::Held && r.owner == session.owner)
            });
        service.acquire(&session.owner, &resources).map_err(error)?;
        if !already_owned && let Some(mut goal) = self.saved_goal(sequence)? {
            if goal.state != GoalState::Acquiring {
                return Err(invalid(
                    "a new control session must start with fresh acquisition",
                ));
            }
            goal.observation_cursor = host.store().latest_sequence().map_err(HostError::Store)?;
            goal.before.clear();
            goal.after.clear();
            goal.source_cursors.clear();
            goal.source_versions.clear();
            goal.wait_ticks = 0;
            self.save_goal(&goal)?;
        }
        Ok(())
    }

    pub(super) fn cleanup_control_sessions(&mut self, host: &Host) -> Result<(), HostError> {
        for sequence in self
            .journal
            .control_cleanup_candidates(self.control_cursor, self.config.max_jobs_per_tick)?
        {
            self.control_cursor = sequence;
            self.cleanup_control(host, sequence)?;
        }
        Ok(())
    }
    pub(super) fn cleanup_control(&self, host: &Host, sequence: u64) -> Result<(), HostError> {
        let Some(session) = self.journal.control_session(sequence)? else {
            return Ok(());
        };
        if session.closed {
            return Ok(());
        }
        let mut evidence = Vec::new();
        if let Some(goal) = self.saved_goal(sequence)? {
            if !goal.state.terminal()
                && !matches!(goal.state, GoalState::WaitingTime | GoalState::WaitingEvent)
            {
                return Ok(());
            }
            if goal.dispatch_cursor.is_some() {
                let Some(outcome) = &goal.outcome else {
                    return Ok(());
                };
                let outcome = host
                    .store()
                    .get(outcome)
                    .map_err(HostError::Store)?
                    .ok_or_else(|| invalid("missing control outcome"))?;
                if outcome.schema_id.0 != "arcs.outcome.v1"
                    || outcome.created_by.actor_type != ActorType::System
                    || outcome.created_by.id != "arcs.outcome"
                    || outcome.source.kind != arcs::core::SourceKind::Internal
                    || outcome.payload["execution_result_version"].as_str()
                        != goal.execution.as_ref().map(|id| id.0.as_str())
                    || !matches!(
                        outcome.payload["verdict"].as_str(),
                        Some("success" | "failure")
                    )
                {
                    return Ok(());
                }
                evidence.push(outcome.version_id);
            } else if let Some(version) = goal.approval.or(goal.assessment) {
                evidence.push(version);
            }
        }
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let held = held_reservations(&service, &session)?
            .into_iter()
            .map(|r| r.version_id)
            .collect::<Vec<_>>();
        if !held.is_empty() {
            service
                .release(
                    &session.owner,
                    &held,
                    &Actor {
                        actor_type: ActorType::System,
                        id: "arcs.host.runtime".into(),
                    },
                    "goal control session closed",
                    &evidence,
                )
                .map_err(error)?;
        }
        self.journal.close_control_session(&session.owner)
    }

    pub fn exclusive_resources(&self, host: &Host) -> Result<Vec<Reservation>, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        let names = self
            .config
            .routes
            .iter()
            .flat_map(|r| {
                r.goal.iter().chain(
                    r.plan
                        .iter()
                        .flat_map(|p| p.templates.iter().filter_map(|t| t.route.goal.as_ref())),
                )
            })
            .flat_map(|g| g.exclusive_resources.iter())
            .collect::<BTreeSet<_>>();
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        names
            .into_iter()
            .map(|r| service.current(r).map_err(error))
            .filter_map(|r| r.transpose())
            .collect()
    }

    /// Authenticated operator acknowledgement for terminal goals with an
    /// unresolved dispatch. This records evidence, but cannot verify hardware idle.
    pub fn reconcile_goal_resources(
        &self,
        host: &Host,
        sequence: u64,
        expected: &[VersionId],
        evidence: &[VersionId],
        actor: Actor,
        reason: String,
    ) -> Result<(), HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        let goal = self
            .saved_goal(sequence)?
            .ok_or_else(|| invalid("unknown goal"))?;
        if !goal.state.terminal() || evidence.is_empty() {
            return Err(invalid(
                "resource reconciliation requires a terminal goal and evidence",
            ));
        }
        let session = self
            .journal
            .control_session(sequence)?
            .ok_or_else(|| invalid("goal has no control session"))?;
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let held = held_reservations(&service, &session)?
            .into_iter()
            .map(|r| r.version_id.0)
            .collect::<HashSet<_>>();
        let wanted = expected.iter().map(|v| v.0.clone()).collect::<HashSet<_>>();
        if held != wanted && !held.is_empty() {
            return Err(invalid(
                "reconciliation must name every current held reservation revision",
            ));
        }
        if held.is_empty() {
            let mut names = HashSet::new();
            for id in expected {
                let artifact = host
                    .store()
                    .get(id)
                    .map_err(HostError::Store)?
                    .ok_or_else(|| invalid("missing expected reservation"))?;
                names.insert(
                    artifact.payload["resource"]
                        .as_str()
                        .ok_or_else(|| invalid("invalid reservation resource"))?
                        .to_owned(),
                );
            }
            if names != session.resources.iter().cloned().collect() {
                return Err(invalid("reconciliation resource set changed"));
            }
        }
        service
            .release(&session.owner, expected, &actor, &reason, evidence)
            .map_err(error)?;
        self.journal.close_control_session(&session.owner)
    }
}

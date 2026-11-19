//! Cooperative exclusive goal rounds in the existing Core, without expiry.
mod release;
mod review;
pub(crate) use release::ControlReleaseIntent;
pub use review::ResourceEvidenceReview;

use super::{
    TaskRuntime,
    config::invalid,
    goal::{GoalConfig, GoalState},
    journal::ControlSession,
};
use crate::{Host, HostError};
use arcs::core::{SystemClock, VersionId};
use arcs::runtime::reservations::{
    Reservation, ReservationError, ReservationService, ReservationState,
};
use std::collections::BTreeSet;

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
        fenced_resource: Option<&str>,
    ) -> Result<Option<Reservation>, HostError> {
        if config.exclusive_resources.is_empty() {
            return Ok(None);
        }
        let session = self
            .journal
            .control_session(sequence)?
            .ok_or_else(|| invalid("missing control session"))?;
        if session.closed {
            return Err(invalid("control session already closed"));
        }
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let mut fence = None;
        for resource in &session.resources {
            let reservation = service
                .current(resource)
                .map_err(error)?
                .filter(|r| r.state == ReservationState::Held && r.owner == session.owner)
                .ok_or_else(|| invalid("goal no longer owns its exclusive resource"))?;
            if fenced_resource == Some(resource.as_str()) {
                fence = Some(reservation);
            }
        }
        if fenced_resource.is_some() && fence.is_none() {
            return Err(invalid("goal does not own the configured fenced resource"));
        }
        Ok(fence)
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
}

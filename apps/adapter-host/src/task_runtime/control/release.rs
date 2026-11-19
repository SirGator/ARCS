//! Goal release decisions are durable; only the controller handshake runs off-thread.
use super::{ControlSession, Host, HostError, TaskRuntime, error, held_reservations, invalid};
use crate::task_runtime::goal::{GoalState, GoalTask};
use arcs::core::{Actor, ActorType, SystemClock, VersionId};
use arcs::runtime::reservations::{
    PreparedReservationRelease, ReservationError, ReservationOwner, ReservationRelease,
    ReservationService,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ControlReleaseIntent {
    pub expected: Vec<VersionId>,
    pub evidence: Vec<VersionId>,
    pub actor: Actor,
    pub reason: String,
}

impl ControlReleaseIntent {
    fn request<'a>(&'a self, owner: &'a ReservationOwner) -> ReservationRelease<'a> {
        ReservationRelease {
            owner,
            expected: &self.expected,
            evidence: &self.evidence,
            actor: &self.actor,
            reason: &self.reason,
        }
    }
}

impl TaskRuntime {
    pub(in crate::task_runtime) fn cleanup_control(
        &self,
        host: &Host,
        sequence: u64,
    ) -> Result<(), HostError> {
        let Some(session) = self.journal.control_session(sequence)? else {
            return Ok(());
        };
        let goal = self.saved_goal(sequence)?;
        if session.closed
            || goal.as_ref().is_some_and(|goal| {
                !goal.state.terminal()
                    && !matches!(goal.state, GoalState::WaitingTime | GoalState::WaitingEvent)
            })
        {
            return Ok(());
        }
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let held: Vec<_> = held_reservations(&service, &session)?
            .into_iter()
            .map(|r| r.version_id)
            .collect();
        if held.is_empty() {
            return self.journal.close_control_session(&session.owner);
        }
        let automatic;
        let intent = match &session.release_intent {
            Some(intent) => intent,
            None => {
                let Some(evidence) = self.automatic_release_evidence(host, goal.as_ref())? else {
                    return Ok(());
                };
                automatic = ControlReleaseIntent {
                    expected: held,
                    evidence,
                    actor: Actor {
                        actor_type: ActorType::System,
                        id: "arcs.host.runtime".into(),
                    },
                    reason: "goal control session closed".into(),
                };
                &automatic
            }
        };
        let prepared =
            self.prepare_control_release(host, goal.as_ref(), intent.request(&session.owner))?;
        match self
            .record_release_result(&session.owner, Self::poll_prepared_release(host, prepared))
        {
            Err(HostError::ControlReleasePending(_)) => Ok(()),
            result => result,
        }
    }

    fn automatic_release_evidence(
        &self,
        host: &Host,
        goal: Option<&GoalTask>,
    ) -> Result<Option<Vec<VersionId>>, HostError> {
        let Some(goal) = goal else {
            return Ok(Some(Vec::new()));
        };
        if goal.dispatch_cursor.is_none() {
            return Ok(Some(
                goal.approval
                    .clone()
                    .or_else(|| goal.assessment.clone())
                    .into_iter()
                    .collect(),
            ));
        }
        let Some(outcome) = &goal.outcome else {
            return Ok(None);
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
            return Ok(None);
        }
        Ok(Some(vec![outcome.version_id]))
    }

    fn prepare_control_release<'host>(
        &self,
        host: &'host Host,
        goal: Option<&GoalTask>,
        release: ReservationRelease<'_>,
    ) -> Result<PreparedReservationRelease<'host>, HostError> {
        let mut configured = Vec::new();
        if let Some(goal) = goal {
            let route = self.raw_route_for_job(host, goal.sequence, &goal.route_id)?;
            let config = route
                .goal
                .ok_or_else(|| invalid("control session route has no goal"))?;
            configured.extend(config.actions.into_iter().filter_map(|action| {
                action
                    .fenced_resource
                    .map(|resource| (resource, action.capability))
            }));
        }
        ReservationService::new(host.store(), host.schemas(), &SystemClock)
            .prepare_release(release, &configured)
            .map_err(error)
    }

    fn poll_prepared_release(
        host: &Host,
        prepared: PreparedReservationRelease<'_>,
    ) -> Result<(), ReservationError> {
        if prepared.controllers().is_empty() {
            return prepared.commit();
        }
        let mut failure = None;
        for closure in prepared.controllers() {
            if let Err(error) = host.start_controller_closure(closure) {
                failure.get_or_insert(error);
            }
        }
        Err(failure.unwrap_or(ReservationError::ControllersPending))
    }

    fn record_release_result(
        &self,
        owner: &ReservationOwner,
        result: Result<(), ReservationError>,
    ) -> Result<(), HostError> {
        match result {
            Ok(()) => self.journal.close_control_session(owner),
            Err(
                failure @ (ReservationError::ControllersPending
                | ReservationError::ControllerAcknowledgement { .. }),
            ) => {
                let detail: String = format!("{failure:?}").chars().take(512).collect();
                self.journal.note_control_release_error(owner, &detail)?;
                Err(HostError::ControlReleasePending(detail))
            }
            Err(failure) => Err(error(failure)),
        }
    }

    /// Synchronous single-command API for a stopped runtime.
    pub fn reconcile_goal_resources(
        &self,
        host: &Host,
        sequence: u64,
        expected: &[VersionId],
        evidence: &[VersionId],
        actor: Actor,
        reason: String,
    ) -> Result<(), HostError> {
        let intent = ControlReleaseIntent {
            expected: expected.to_vec(),
            evidence: evidence.to_vec(),
            actor,
            reason,
        };
        let (goal, session) = self.reconciliation_context(host, sequence, &intent)?;
        let prepared =
            self.prepare_control_release(host, Some(&goal), intent.request(&session.owner))?;
        self.record_release_result(
            &session.owner,
            prepared.commit_with_controllers(|capability| host.reservation_controller(capability)),
        )
    }

    /// Accept an operator decision into the running scheduler. True means pending.
    pub(crate) fn queue_goal_reconciliation(
        &self,
        host: &Host,
        sequence: u64,
        intent: ControlReleaseIntent,
    ) -> Result<bool, HostError> {
        let (goal, session) = self.reconciliation_context(host, sequence, &intent)?;
        let prepared =
            self.prepare_control_release(host, Some(&goal), intent.request(&session.owner))?;
        let pending = !prepared.controllers().is_empty();
        let result = if pending {
            self.journal
                .save_control_release_intent(&session.owner, &intent)?;
            Self::poll_prepared_release(host, prepared)
        } else {
            prepared.commit()
        };
        if !pending && matches!(result, Err(ReservationError::ControllersPending)) {
            // Also covers a new controller registered between preparation and commit.
            self.journal
                .save_control_release_intent(&session.owner, &intent)?;
        }
        match self.record_release_result(&session.owner, result) {
            Ok(()) => Ok(false),
            Err(HostError::ControlReleasePending(_)) => Ok(true),
            Err(error) => Err(error),
        }
    }

    fn reconciliation_context(
        &self,
        host: &Host,
        sequence: u64,
        intent: &ControlReleaseIntent,
    ) -> Result<(GoalTask, ControlSession), HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        let goal = self
            .saved_goal(sequence)?
            .ok_or_else(|| invalid("unknown goal"))?;
        if !goal.state.terminal() || intent.evidence.is_empty() {
            return Err(invalid(
                "resource reconciliation requires a terminal goal and evidence",
            ));
        }
        let session = self
            .journal
            .control_session(sequence)?
            .ok_or_else(|| invalid("goal has no control session"))?;
        if session
            .release_intent
            .as_ref()
            .is_some_and(|stored| stored != intent)
        {
            return Err(invalid(
                "an operator release decision is already recorded for this session",
            ));
        }
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let held: HashSet<_> = held_reservations(&service, &session)?
            .into_iter()
            .map(|r| r.version_id)
            .collect();
        if held.is_empty() {
            let mut names = HashSet::new();
            for id in &intent.expected {
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
        } else if held != intent.expected.iter().cloned().collect() {
            return Err(invalid(
                "reconciliation must name every current held reservation revision",
            ));
        }
        Ok((goal, session))
    }
}

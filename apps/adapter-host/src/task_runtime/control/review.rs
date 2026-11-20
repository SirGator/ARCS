//! Operator views of held reservations and immutable reconciliation evidence.
use super::{ControlSession, Host, HostError, TaskRuntime, held_reservations, invalid};
use crate::task_runtime::goal::{GoalState, GoalTask};
use arcs::core::{Artifact, SystemClock, VersionId};
use arcs::execution::{ExecutionError, execution_invocation_id};
use arcs::runtime::reservations::{Reservation, ReservationService, ReservationState};
use arcs::runtime::{InvocationError, InvocationKind, InvocationService, InvocationStatus};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(crate) struct ResourceReleaseReview {
    sequence: u64,
    route_id: String,
    state: GoalState,
    execution: Option<VersionId>,
    outcome: Option<VersionId>,
    reservations: Vec<ResourceRevision>,
    release_requested: bool,
}

#[derive(Serialize)]
struct ResourceRevision {
    resource: String,
    version_id: VersionId,
}

#[derive(Serialize)]
pub struct ResourceEvidenceReview {
    sequence: u64,
    reservation_versions: Vec<VersionId>,
    artifacts: Vec<Artifact>,
}

impl TaskRuntime {
    fn terminal_control_session(
        &self,
        sequence: u64,
    ) -> Result<Option<(GoalTask, ControlSession)>, HostError> {
        let Some(session) = self
            .journal
            .control_session(sequence)?
            .filter(|session| !session.closed)
        else {
            return Ok(None);
        };
        let Some(goal) = self.saved_goal(sequence)? else {
            return Ok(None);
        };
        Ok((goal.sequence == sequence
            && goal.state.terminal()
            && goal.input_version_id == session.owner.input_version)
            .then_some((goal, session)))
    }

    pub(crate) fn resource_release_reviews(
        &self,
        host: &Host,
        reservations: &[Reservation],
    ) -> Result<Vec<ResourceReleaseReview>, HostError> {
        if !reservations
            .iter()
            .any(|r| r.state == ReservationState::Held)
        {
            return Ok(Vec::new());
        }
        let definition = self.journal.definition_version()?;
        let mut by_task = BTreeMap::<_, Vec<_>>::new();
        for record in reservations {
            if record.state == ReservationState::Held
                && record.owner.definition_version == definition
            {
                by_task
                    .entry(record.owner.task_sequence)
                    .or_default()
                    .push(record);
            }
        }
        let mut reviews = Vec::new();
        for (sequence, records) in by_task {
            if let Some((goal, session)) = self.terminal_control_session(sequence)? {
                let reservations: Vec<_> = records
                    .into_iter()
                    .filter(|record| record.owner == session.owner)
                    .map(|record| ResourceRevision {
                        resource: record.resource.clone(),
                        version_id: record.version_id.clone(),
                    })
                    .collect();
                if !reservations.is_empty() {
                    let execution = execution_evidence(host, &goal)?;
                    reviews.push(ResourceReleaseReview {
                        sequence,
                        route_id: goal.route_id,
                        state: goal.state,
                        execution,
                        outcome: goal.outcome,
                        reservations,
                        release_requested: session.release_intent.is_some(),
                    });
                }
            }
        }
        Ok(reviews)
    }

    pub(crate) fn inspect_resource_evidence(
        &self,
        host: &Host,
        sequence: u64,
        reservation_versions: Vec<VersionId>,
        evidence_versions: &[VersionId],
    ) -> Result<ResourceEvidenceReview, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        if !(1..=i64::MAX as u64).contains(&sequence)
            || !(1..=16).contains(&reservation_versions.len())
            || !(1..=32).contains(&evidence_versions.len())
        {
            return Err(invalid("invalid resource evidence review request"));
        }
        let (_, session) = self.terminal_control_session(sequence)?.ok_or_else(|| {
            invalid("resource review requires a terminal goal with an open session")
        })?;
        let service = ReservationService::new(host.store(), host.schemas(), &SystemClock);
        let held = held_reservations(&service, &session)?;
        if held.len() != reservation_versions.len()
            || !held
                .iter()
                .all(|record| reservation_versions.contains(&record.version_id))
        {
            return Err(invalid("held reservation revisions have changed"));
        }

        let mut review = ResourceEvidenceReview {
            sequence,
            reservation_versions,
            artifacts: Vec::new(),
        };
        let encoded_size = |value: &ResourceEvidenceReview| {
            serde_json::to_vec(value)
                .map(|bytes| bytes.len())
                .map_err(|error| invalid(error.to_string()))
        };
        let mut size = encoded_size(&review)?;
        for version in evidence_versions {
            let artifact = host
                .store()
                .get(version)
                .map_err(HostError::Store)?
                .ok_or_else(|| invalid("missing reconciliation evidence"))?;
            size += serde_json::to_vec(&artifact)
                .map_err(|error| invalid(error.to_string()))?
                .len()
                + usize::from(!review.artifacts.is_empty());
            if size > 65_536 {
                return Err(invalid(
                    "resource evidence review exceeds the local preview size limit",
                ));
            }
            review.artifacts.push(artifact);
        }
        Ok(review)
    }
}

// Late replies are visible as evidence without advancing the terminal task.
fn execution_evidence(host: &Host, goal: &GoalTask) -> Result<Option<VersionId>, HostError> {
    if let Some(execution) = &goal.execution {
        return Ok(Some(execution.clone()));
    }
    let (Some(action), Some(capability)) = (&goal.action, &goal.capability) else {
        return Ok(None);
    };
    let Ok((_, descriptor)) = host
        .registry()
        .authorized_capability(&capability.adapter_id, &capability.capability_id)
    else {
        return Ok(None);
    };
    let Some([schema]) = descriptor.contract.act().map(|contract| contract.emits) else {
        return Ok(None);
    };
    let id = execution_invocation_id(action, capability, schema);
    let failure = |error| HostError::Execution {
        adapter: capability.adapter_id.clone(),
        source: ExecutionError::Invocation(error),
    };
    let invocation = InvocationService::new(host.store(), host.schemas(), &SystemClock)
        .lookup(&id)
        .map_err(failure)?;
    let Some(invocation) = invocation else {
        return Ok(None);
    };
    if invocation.kind != InvocationKind::Execution || &invocation.input_version != action {
        return Err(failure(InvocationError::IdentityConflict(id)));
    }
    if invocation.status == InvocationStatus::Succeeded {
        return invocation
            .result_version
            .map(Some)
            .ok_or_else(|| failure(InvocationError::MissingResult));
    }
    Ok(None)
}

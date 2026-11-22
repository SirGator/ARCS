//! Operational ownership of Core recovery decisions; no second truth store.
use super::*;
use arcs::adapters::input_data::InputDataSelection;
use arcs::core::ActorType;
use arcs::reasoning::recovery::{RecoveryCall, RecoveryDefinition, RecoveryPreparation};
use arcs::reasoning::{ReasoningRequestV2, selection::ReasonerChoice};

impl TaskRuntime {
    pub(super) fn recovery_now(&self, tick_time: u64) -> Result<u64, HostError> {
        if self.live_clock {
            scheduler::now_unix_ms()
        } else {
            Ok(tick_time)
        }
    }
    pub(super) fn recovery_definition(
        &self,
        host: &Host,
        sequence: u64,
        request: &ReasoningRequestV2,
        inputs: &[InputDataSelection],
    ) -> Result<Option<RecoveryDefinition>, HostError> {
        if !self.has_reasoning_recovery {
            return Ok(None);
        }
        let (id, input) = self.journal.selection_job(sequence)?;
        let route = self.route_for_job(host, sequence, &id)?;
        let Some(policy) = route.reasoning_recovery else {
            return Ok(None);
        };
        // Only the planning request inherits recovery on a goal route.
        if route.goal.is_some()
            && self
                .saved_goal(sequence)?
                .is_none_or(|task| request.request_id != self.goal_request_id(&task, "plan"))
        {
            return Ok(None);
        }
        if request.capability != route.reasoner
            || request.method != route.method
            || request.budget != route.budget
            || request.target_schema_id != route.target_schema_id
        {
            return Ok(None);
        }
        Ok(Some(RecoveryDefinition {
            owner: self.selection_owner(sequence, input)?,
            base_request: request.clone(),
            inputs: inputs.to_vec(),
            policy,
        }))
    }

    pub(super) fn prepare_recovery(
        host: &mut Host,
        definition: &RecoveryDefinition,
        now: u64,
    ) -> Result<RecoveryCall, HostError> {
        match host.prepare_reasoning_recovery(definition, now)? {
            RecoveryPreparation::Ready(call) => Ok(*call),
            RecoveryPreparation::Waiting { not_before_unix_ms } => Err(HostError::ResourceBusy {
                resource: "reasoning-recovery-delay".into(),
                requested: not_before_unix_ms,
                available: now,
            }),
            RecoveryPreparation::Stopped {
                failure_version_id,
                attempts,
            } => Err(invalid(format!(
                "reasoning recovery stopped after {attempts} attempts; failure evidence {}",
                failure_version_id.0
            ))),
        }
    }

    pub(super) fn completed_reasoner(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        completion: &CompletedTask,
    ) -> Result<ReasonerChoice, HostError> {
        let Some(policy) = &route.reasoning_recovery else {
            return Ok(ReasonerChoice {
                capability: route.reasoner.clone(),
                method: route.method,
                budget: route.budget.clone(),
            });
        };
        let owner =
            self.selection_owner(completion.sequence, completion.input_version_id.clone())?;
        let revision = if route.plan.as_ref().is_some_and(|p| p.replanning.is_some()) {
            self.journal
                .planner_request_revision(completion.sequence, &completion.request_version_id)?
        } else {
            0
        };
        let request_id = self.task_request_id(route, &completion.input_version_id, revision);
        arcs::reasoning::recovery::completed_choice(
            host.registry(),
            host.schemas(),
            host.store(),
            &completion.request_version_id,
            &owner,
            policy,
            &request_id,
        )
        .map_err(HostError::Reasoning)
    }
}

#[derive(Debug, Serialize)]
pub struct TaskRecoveryReport {
    pub sequence: u64,
    pub route_id: String,
    pub recovery: arcs::reasoning::recovery::RecoveryStatus,
}
impl TaskRuntime {
    pub fn reasoning_recoveries(&self, host: &Host) -> Result<Vec<TaskRecoveryReport>, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        if !self.has_reasoning_recovery {
            return Ok(vec![]);
        }
        self.journal
            .recovery_report_jobs()?
            .into_iter()
            .map(|(sequence, route_id, definition)| {
                let (_, input) = self.journal.selection_job(sequence)?;
                let owner = self.selection_owner(sequence, input)?;
                let route = self.raw_route_for_job(host, sequence, &route_id)?;
                let policy = route
                    .reasoning_recovery
                    .as_ref()
                    .ok_or_else(|| invalid("recovery report has no policy"))?;
                let recovery = arcs::reasoning::recovery::status(
                    host.registry(),
                    host.schemas(),
                    host.store(),
                    &definition,
                    &owner,
                    policy,
                )
                .map_err(HostError::Reasoning)?;
                Ok(TaskRecoveryReport {
                    sequence,
                    route_id,
                    recovery,
                })
            })
            .collect()
    }
}

impl TaskRuntime {
    pub(super) fn goal_candidate_reasoner(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        task: &super::goal::GoalTask,
        candidate: &Artifact,
    ) -> Result<ReasonerChoice, HostError> {
        let binding = self
            .saved_reasoner_for_task(host, task.sequence)?
            .filter(|binding| binding.round.is_some());
        if binding.is_none()
            && route
                .reasoner_selection
                .as_ref()
                .is_some_and(|routing| routing.reselect_each_round)
        {
            return Err(invalid("missing reasoner selection for this goal round"));
        }
        let direct = ReasonerChoice {
            capability: route.reasoner.clone(),
            method: route.method,
            budget: route.budget.clone(),
        };
        let recovery = route.reasoning_recovery.as_ref();
        if binding.is_none() && recovery.is_none() {
            return Ok(direct);
        }
        let generated = arcs::store::ArtifactRelations::new(host.store())
            .outgoing(&candidate.version_id)
            .map_err(HostError::Store)?
            .into_iter()
            .filter(|edge| edge.kind == arcs::store::relation_kinds::generated_by())
            .map(|edge| edge.to)
            .collect::<Vec<_>>();
        let [origin] = generated.as_slice() else {
            return Err(invalid("goal candidate needs one originating request"));
        };
        let request_id = self.goal_request_id(task, "plan");
        if let Some(binding) = binding {
            let request = host
                .store()
                .get(origin)
                .map_err(HostError::Store)?
                .ok_or_else(|| invalid("missing originating goal request"))?;
            if request.schema_id.0 != "arcs.reasoning_request.v2"
                || request.created_by.id != "arcs.reasoning.v2"
                || request.created_by.actor_type != ActorType::System
                || request
                    .provenance
                    .as_ref()
                    .is_none_or(|p| !p.parents.contains(&binding.selection.version_id.0))
                || (recovery.is_none() && request.payload["invocation"]["request_id"] != request_id)
            {
                return Err(invalid(
                    "goal candidate does not belong to this round's reasoner selection",
                ));
            }
        }
        let Some(policy) = recovery else {
            return Ok(direct);
        };
        let owner = self.selection_owner(task.sequence, task.input_version_id.clone())?;
        arcs::reasoning::recovery::completed_choice(
            host.registry(),
            host.schemas(),
            host.store(),
            origin,
            &owner,
            policy,
            &request_id,
        )
        .map_err(HostError::Reasoning)
    }
}

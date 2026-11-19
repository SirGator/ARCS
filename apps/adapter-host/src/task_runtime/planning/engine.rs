use super::*;
use crate::task_runtime::{CompletedTask, TaskRuntime, config::invalid};
use crate::{Host, HostError};
use arcs::core::Artifact;
use arcs::reasoning::{ContextSelection, ReasoningRequestV2, ReasoningStatus};

impl TaskRuntime {
    pub(in crate::task_runtime) fn route_for_job(
        &self,
        host: &Host,
        sequence: u64,
        id: &str,
    ) -> Result<ReasoningRoute, HostError> {
        let route = self.raw_route_for_job(host, sequence, id)?;
        self.apply_saved_reasoner(host, sequence, route)
    }
    pub(in crate::task_runtime) fn raw_route_for_job(
        &self,
        host: &Host,
        sequence: u64,
        id: &str,
    ) -> Result<ReasoningRoute, HostError> {
        if let Some(route) = self.config.routes.iter().find(|r| r.id == id) {
            return Ok(route.clone());
        }
        let (parent, version, node, input) = self
            .journal
            .dynamic_origin(sequence)?
            .ok_or_else(|| invalid("unknown saved route"))?;
        let plan = self.load_plan(host, &version)?;
        if plan.parent_sequence != parent || plan.input_version_id != input {
            return Err(invalid("plan instance identity changed"));
        }
        self.validate_frozen_plan(host, &version, &plan)?;
        plan.steps
            .into_iter()
            .find(|s| s.id == node && s.route.id == id)
            .map(|s| s.route)
            .ok_or_else(|| invalid("saved plan step is missing"))
    }
    pub(in crate::task_runtime) fn advance_plan(
        &mut self,
        host: &mut Host,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
        now: u64,
    ) -> Result<(), HostError> {
        let config = route
            .plan
            .as_ref()
            .ok_or_else(|| invalid("planning route is missing configuration"))?;
        let pending = self.journal.pending_replan(sequence)?;
        if pending.is_none()
            && let Some(saved) = self.journal.saved_plan(sequence)?
        {
            if saved.state != "accepted" {
                return Ok(());
            }
            let version = saved
                .plan_version
                .as_ref()
                .ok_or_else(|| invalid("accepted plan has no artifact"))?;
            let plan = self.load_plan(host, version)?;
            self.validate_frozen_plan(host, version, &plan)?;
            let states = self.journal.plan_step_states(sequence, version)?;
            if states.len() != plan.steps.len() {
                return Err(invalid("accepted plan has incomplete operational steps"));
            }
            let mut waiting = false;
            let mut blocked = None;
            for task in states {
                match self.resolve_parent(host, task)? {
                    crate::task_runtime::dependencies::ParentResult::Waiting => waiting = true,
                    crate::task_runtime::dependencies::ParentResult::Blocked(reason) => {
                        blocked = Some(reason);
                        break;
                    }
                    crate::task_runtime::dependencies::ParentResult::Ready(_) => {}
                }
            }
            if blocked.is_some()
                && config.replanning.as_ref().is_some_and(|p| {
                    plan.revision
                        .as_ref()
                        .is_some_and(|r| r.index < p.max_revisions)
                })
            {
                if self
                    .revision_budget(host, sequence, config, route, &[])?
                    .is_none()
                {
                    self.journal.finish_plan(
                        &saved.completion,
                        Some("plan aggregate budget exhausted before replanning"),
                        true,
                    )?;
                    self.reap_abandoned_task_work(host)?;
                    return Ok(());
                }
                let context = self.make_replanning_context(host, route, &plan, version)?;
                self.journal.begin_replan(&context)?;
                self.reap_abandoned_task_work(host)?;
                self.cleanup_control_sessions(host)?;
            } else if blocked.is_some() || !waiting {
                self.journal.finish_plan(
                    &saved.completion,
                    blocked.as_deref(),
                    config.replanning.is_some(),
                )?;
                self.reap_abandoned_task_work(host)?;
            }
            return Ok(());
        }
        self.journal.start(sequence)?;
        let mut constraints = route.constraints.clone();
        if let Some(object) = constraints.as_object_mut() {
            object.insert("task_planning".into(), config.catalog(host)?);
        } else {
            constraints = serde_json::json!({"operator_constraints":constraints,"task_planning":config.catalog(host)?});
        }
        let (revision, context, meta) = if let Some(pending) = pending {
            let artifact = self.publish_replanning_context(host, &pending.context)?;
            if pending
                .version
                .as_ref()
                .is_some_and(|v| v != &artifact.version_id)
            {
                return Err(invalid("replanning artifact identity changed"));
            }
            self.journal.bind_replan_context(
                sequence,
                pending.context.revision,
                &artifact.version_id,
            )?;
            constraints["task_replanning"] = serde_json::json!({"revision":pending.context.revision,"previous_plan_version_id":pending.context.previous_plan_version_id,"context_version_id":artifact.version_id,"remaining_budget":self.revision_budget(host,sequence,config,route,&[])?});
            let inputs =
                self.replanning_inputs(host, input, route, &pending.context, &artifact.version_id)?;
            (
                pending.context.revision,
                inputs,
                Some(PlanRevision {
                    index: pending.context.revision,
                    previous_plan_version_id: Some(pending.context.previous_plan_version_id),
                    context_version_id: Some(artifact.version_id),
                }),
            )
        } else {
            (
                0,
                vec![ContextSelection {
                    version_id: input.version_id.clone(),
                    payload_fields: route.payload_fields.clone(),
                }],
                config.replanning.as_ref().map(|_| PlanRevision {
                    index: 0,
                    previous_plan_version_id: None,
                    context_version_id: None,
                }),
            )
        };
        let request = ReasoningRequestV2 {
            request_id: self.task_request_id(route, &input.version_id, revision),
            capability: route.reasoner.clone(),
            method: route.method,
            objective: route.objective.clone(),
            context,
            target_schema_id: route.target_schema_id.clone(),
            allowed_capabilities: vec![],
            constraints,
            budget: route.budget.clone(),
        };
        let Some(result) =
            self.reason_for_task(host, sequence, request, &[], route.memory.as_ref(), now)?
        else {
            return Ok(());
        };
        let completion = CompletedTask {
            sequence,
            route_id: route.id.clone(),
            input_version_id: input.version_id.clone(),
            reasoning_status: result.status,
            request_version_id: result.request_version_id,
            result_version_id: result.result_version_id,
            candidate_version_ids: result
                .candidates
                .iter()
                .map(|c| c.version_id.clone())
                .collect(),
        };
        if result.status != ReasoningStatus::Solved || result.candidates.len() != 1 {
            return self
                .journal
                .reject_plan(&completion, "planner produced no single solved plan");
        }
        let proposal = &result.candidates[0];
        let steps = serde_json::from_value::<PlanProposal>(proposal.payload.clone())
            .map_err(|e| invalid(e.to_string()))
            .and_then(|p| config.compile_revision(sequence, &p, &self.config.routes, revision));
        let steps = match steps {
            Ok(steps) => steps,
            Err(error) => return self.journal.reject_plan(&completion, &error.to_string()),
        };
        if config.replanning.is_some()
            && self
                .revision_budget(host, sequence, config, route, &steps)?
                .is_none()
        {
            return self.journal.reject_plan(
                &completion,
                "plan cumulative reasoning or action budget exceeded",
            );
        }
        let plan = AcceptedPlan {
            runtime_id: self.config.runtime_id.clone(),
            parent_sequence: sequence,
            parent_route_id: route.id.clone(),
            input_version_id: input.version_id.clone(),
            proposal_version_id: proposal.version_id.clone(),
            steps,
            revision: meta,
        };
        if serde_json::to_vec(&plan)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > 60_000
        {
            return self
                .journal
                .reject_plan(&completion, "accepted plan snapshot exceeds 60000 bytes");
        }
        let artifact = self.publish_plan(host, &plan)?;
        self.journal
            .admit_plan(&plan, &artifact.version_id, &completion, now)
    }
    pub fn plans_after(&self, sequence: u64, limit: usize) -> Result<Vec<PlanReceipt>, HostError> {
        self.journal.plans_after(sequence, limit)
    }
}

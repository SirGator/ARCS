use super::*;
use crate::task_runtime::{CompletedTask, TaskRuntime, config::invalid};
use crate::{Host, HostError};
use arcs::core::Artifact;
use arcs::reasoning::{ContextSelection, ReasoningRequestV2, ReasoningStatus};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};

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
        if let Some(route) = self.config.routes.iter().find(|route| route.id == id) {
            return Ok(route.clone());
        }
        let (parent, version, node, input) = self
            .journal
            .dynamic_origin(sequence)?
            .ok_or_else(|| invalid("unknown saved route"))?;
        let plan = self.load_plan(host, &version)?;
        if plan.parent_sequence != parent
            || plan.runtime_id != self.config.runtime_id
            || plan.input_version_id != input
        {
            return Err(invalid("plan instance identity changed"));
        }
        let owner = self
            .config
            .routes
            .iter()
            .find(|route| route.id == plan.parent_route_id)
            .ok_or_else(|| invalid("plan owner route is missing"))?;
        let owner = self.apply_saved_reasoner(host, parent, owner.clone())?;
        let config = owner
            .plan
            .as_ref()
            .ok_or_else(|| invalid("plan owner no longer authorizes planning"))?;
        let candidate = host
            .store()
            .get(&plan.proposal_version_id)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("plan proposal artifact is missing"))?;
        let producer = if owner.reasoning_recovery.is_some() {
            let saved = self
                .journal
                .saved_plan(parent)?
                .ok_or_else(|| invalid("missing planner completion for recovery"))?;
            if saved.completion.sequence != parent
                || saved.completion.route_id != owner.id
                || saved.completion.input_version_id != plan.input_version_id
                || !saved
                    .completion
                    .candidate_version_ids
                    .contains(&plan.proposal_version_id)
            {
                return Err(invalid("planner recovery completion changed"));
            }
            self.completed_reasoner(host, &owner, &saved.completion)?
                .capability
                .adapter_id
                .0
        } else {
            owner.reasoner.adapter_id.0.clone()
        };
        if candidate.schema_id.0 != PLAN_CANDIDATE || candidate.created_by.id != producer {
            return Err(invalid("plan proposal origin changed"));
        }
        let proposal: PlanProposal =
            serde_json::from_value(candidate.payload).map_err(|e| invalid(e.to_string()))?;
        let expected = config.compile(parent, &proposal, &self.config.routes)?;
        if serde_json::to_value(&expected).map_err(|e| invalid(e.to_string()))?
            != serde_json::to_value(&plan.steps).map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid(
                "plan snapshot exceeds its authorized template definitions",
            ));
        }
        plan.steps
            .into_iter()
            .find(|step| step.id == node && step.route.id == id)
            .map(|step| step.route)
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
        if let Some(saved) = self.journal.saved_plan(sequence)? {
            if saved.state != "accepted" {
                return Ok(());
            }
            let plan = self.load_plan(
                host,
                saved
                    .plan_version
                    .as_ref()
                    .ok_or_else(|| invalid("accepted plan has no artifact"))?,
            )?;
            if plan.parent_sequence != sequence || plan.input_version_id != input.version_id {
                return Err(invalid("accepted plan input identity changed"));
            }
            let states = self.journal.plan_step_states(sequence)?;
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
            if blocked.is_some() || !waiting {
                self.journal
                    .finish_plan(&saved.completion, blocked.as_deref())?;
            }
            return Ok(());
        }
        let config = route
            .plan
            .as_ref()
            .ok_or_else(|| invalid("planning route is missing configuration"))?;
        self.journal.start(sequence)?;
        let mut constraints = route.constraints.clone();
        if let Some(object) = constraints.as_object_mut() {
            object.insert("task_planning".into(), config.catalog(host)?);
        } else {
            constraints = serde_json::json!({"operator_constraints":constraints,"task_planning":config.catalog(host)?});
        }
        let request = ReasoningRequestV2 {
            request_id: deterministic_invocation_id(
                InvocationKind::Reasoning,
                &[
                    "task-runtime",
                    &self.config.runtime_id,
                    &route.id,
                    &input.version_id.0,
                ],
            ),
            capability: route.reasoner.clone(),
            method: route.method,
            objective: route.objective.clone(),
            context: vec![ContextSelection {
                version_id: input.version_id.clone(),
                payload_fields: route.payload_fields.clone(),
            }],
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
            .and_then(|proposal| config.compile(sequence, &proposal, &self.config.routes));
        let steps = match steps {
            Ok(steps) => steps,
            Err(error) => return self.journal.reject_plan(&completion, &error.to_string()),
        };
        let plan = AcceptedPlan {
            runtime_id: self.config.runtime_id.clone(),
            parent_sequence: sequence,
            parent_route_id: route.id.clone(),
            input_version_id: input.version_id.clone(),
            proposal_version_id: proposal.version_id.clone(),
            steps,
        };
        // Reject oversized snapshots before Core publication or child admission.
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

//! Revisions retain the original input and freeze only operator-authorized exports.
use super::*;
use crate::task_runtime::{
    ReasoningRoute, TaskRuntime,
    config::invalid,
    dependencies::{ParentResult, ParentTask, failed_goal_receipt},
};
use crate::{Host, HostError};
use arcs::core::{Artifact, VersionId};
use arcs::reasoning::ContextSelection;
use arcs::runtime::{InvocationKind, deterministic_invocation_id};

impl TaskRuntime {
    pub(in crate::task_runtime) fn task_request_id(
        &self,
        route: &ReasoningRoute,
        input: &VersionId,
        revision: u8,
    ) -> String {
        if revision == 0 {
            deterministic_invocation_id(
                InvocationKind::Reasoning,
                &["task-runtime", &self.config.runtime_id, &route.id, &input.0],
            )
        } else {
            deterministic_invocation_id(
                InvocationKind::Reasoning,
                &[
                    "task-runtime",
                    &self.config.runtime_id,
                    &route.id,
                    &input.0,
                    "plan-revision",
                    &revision.to_string(),
                ],
            )
        }
    }
    pub(super) fn validate_frozen_plan(
        &self,
        host: &Host,
        version: &VersionId,
        plan: &AcceptedPlan,
    ) -> Result<(), HostError> {
        let (route_id, input) = self.journal.selection_job(plan.parent_sequence)?;
        if plan.runtime_id != self.config.runtime_id
            || route_id != plan.parent_route_id
            || input != plan.input_version_id
        {
            return Err(invalid("plan instance identity changed"));
        }
        let owner = self
            .config
            .routes
            .iter()
            .find(|r| r.id == plan.parent_route_id)
            .ok_or_else(|| invalid("plan owner route is missing"))?;
        let owner = self.apply_saved_reasoner(host, plan.parent_sequence, owner.clone())?;
        let config = owner
            .plan
            .as_ref()
            .ok_or_else(|| invalid("plan owner no longer authorizes planning"))?;
        if config.replanning.is_some() != plan.revision.is_some() {
            return Err(invalid("plan revision policy changed"));
        }
        let revision = plan.revision.as_ref().map_or(0, |r| r.index);
        if let Some(meta) = &plan.revision {
            let history = self.journal.plan_revisions(plan.parent_sequence)?;
            let entry = history
                .iter()
                .find(|r| r.index == revision && &r.plan_version_id == version)
                .ok_or_else(|| invalid("missing plan generation binding"))?;
            if entry.proposal_version_id != plan.proposal_version_id
                || entry.context_version_id != meta.context_version_id
                || revision > config.replanning.as_ref().unwrap().max_revisions
            {
                return Err(invalid("plan generation identity changed"));
            }
            let previous = if revision == 0 {
                None
            } else {
                Some(
                    history
                        .iter()
                        .find(|r| r.index == revision - 1)
                        .ok_or_else(|| invalid("missing predecessor generation"))?
                        .plan_version_id
                        .clone(),
                )
            };
            if meta.previous_plan_version_id != previous
                || (revision == 0) != meta.context_version_id.is_none()
            {
                return Err(invalid("plan predecessor identity changed"));
            }
        }
        let candidate = host
            .store()
            .get(&plan.proposal_version_id)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("plan proposal artifact is missing"))?;
        let producer = if owner.reasoning_recovery.is_some() {
            let completion = self
                .journal
                .plan_completion(plan.parent_sequence, version)?;
            if completion.sequence != plan.parent_sequence
                || completion.route_id != owner.id
                || completion.input_version_id != plan.input_version_id
                || !completion
                    .candidate_version_ids
                    .contains(&plan.proposal_version_id)
            {
                return Err(invalid("planner recovery completion changed"));
            }
            self.completed_reasoner(host, &owner, &completion)?
                .capability
                .adapter_id
                .0
        } else {
            owner.reasoner.adapter_id.0.clone()
        };
        if candidate.schema_id != owner.target_schema_id || candidate.created_by.id != producer {
            return Err(invalid("plan proposal origin changed"));
        }
        let proposal: PlanProposal =
            serde_json::from_value(candidate.payload).map_err(|e| invalid(e.to_string()))?;
        let previous = if proposal.steps.iter().any(|step| {
            step.dependencies
                .iter()
                .any(|edge| edge.previous_revision || edge.previous_failure)
        }) {
            let meta = plan
                .revision
                .as_ref()
                .ok_or_else(|| invalid("previous dependencies need a versioned plan"))?;
            let context = self.load_replanning_context(
                host,
                meta.context_version_id.as_ref().ok_or_else(|| {
                    invalid("previous dependencies need a frozen replanning context")
                })?,
            )?;
            if context.runtime_id != plan.runtime_id
                || context.parent_sequence != plan.parent_sequence
                || context.input_version_id != plan.input_version_id
                || context.revision != revision
                || Some(&context.previous_plan_version_id) != meta.previous_plan_version_id.as_ref()
            {
                return Err(invalid(
                    "previous dependency context belongs to another plan",
                ));
            }
            Some(context)
        } else {
            None
        };
        let expected = config.compile_revision(
            plan.parent_sequence,
            &proposal,
            &self.config.routes,
            revision,
            previous.as_ref(),
        )?;
        if serde_json::to_value(expected).map_err(|e| invalid(e.to_string()))?
            != serde_json::to_value(&plan.steps).map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid(
                "plan snapshot exceeds its authorized template definitions",
            ));
        }
        Ok(())
    }
    pub(super) fn make_replanning_context(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        plan: &AcceptedPlan,
        version: &VersionId,
    ) -> Result<ReplanningContext, HostError> {
        let config = route
            .plan
            .as_ref()
            .ok_or_else(|| invalid("missing planning configuration"))?;
        let revision = plan
            .revision
            .as_ref()
            .ok_or_else(|| invalid("replanning requires a versioned plan"))?
            .index
            .checked_add(1)
            .ok_or_else(|| invalid("plan revision overflow"))?;
        let mut result = ReplanningContext {
            runtime_id: self.config.runtime_id.clone(),
            parent_sequence: plan.parent_sequence,
            input_version_id: plan.input_version_id.clone(),
            revision,
            previous_plan_version_id: version.clone(),
            previous_proposal_version_id: plan.proposal_version_id.clone(),
            steps: vec![],
            context: vec![],
            omitted_output_items: 0,
        };
        let minimum = route
            .permitted_reasoner_choices()
            .iter()
            .map(|c| c.budget.max_context_items)
            .min()
            .unwrap_or(0);
        let dependency_items = self
            .journal
            .dependency_bindings(plan.parent_sequence)?
            .iter()
            .map(|b| b.context.len())
            .sum::<usize>();
        let limit = minimum
            .saturating_sub(
                3 + usize::from(config.include_template_availability)
                    + dependency_items
                    + route.memory.as_ref().map_or(0, |p| p.max_items),
            )
            .min(128);
        let tasks = self
            .journal
            .plan_step_states(plan.parent_sequence, version)?;
        if tasks.len() != plan.steps.len() {
            return Err(invalid("accepted plan has incomplete operational steps"));
        }
        let mut failures = Vec::new();
        let mut outputs = Vec::new();
        for mut task in tasks {
            let step = plan
                .steps
                .iter()
                .find(|s| s.route.id == task.route_id)
                .ok_or_else(|| invalid("unknown previous plan step"))?;
            let template = export_template(config, step)?;
            let failure = failure_selection(host, &task, template)?;
            task.fields = template.export_fields.clone();
            let mut summary = ReplanningStep {
                sequence: task.sequence,
                id: step.id.clone(),
                template_id: step.template_id.clone(),
                state: task.state.clone(),
                resolution: "waiting".into(),
                output_version_ids: vec![],
                result_version_id: None,
                failure_receipt_version_id: None,
            };
            match self.resolve_parent(host, task)? {
                ParentResult::Ready(binding) => {
                    summary.resolution = "ready".into();
                    summary.output_version_ids = binding
                        .context
                        .iter()
                        .map(|c| c.version_id.clone())
                        .collect();
                    summary.result_version_id = Some(binding.result_version_id);
                    outputs.extend(binding.context);
                }
                ParentResult::Blocked(_) => {
                    summary.resolution = "blocked".into();
                    if let Some(item) = failure {
                        failures.push((result.steps.len(), item));
                    }
                }
                ParentResult::Waiting => {}
            }
            result.steps.push(summary);
        }
        // Give assessed failures the available slots before optional
        // completed outputs, retaining deterministic step order within each.
        for (index, item) in failures {
            let version = item.version_id.clone();
            if admit_export(&mut result, item, limit) {
                result.steps[index].failure_receipt_version_id = Some(version);
            }
        }
        for item in outputs {
            admit_export(&mut result, item, limit);
        }
        Ok(result)
    }
    pub(super) fn validate_replanning_context(
        &self,
        host: &Host,
        input: &Artifact,
        route: &ReasoningRoute,
        context: &ReplanningContext,
    ) -> Result<(), HostError> {
        if context.runtime_id != self.config.runtime_id
            || context.parent_sequence == 0
            || context.input_version_id != input.version_id
        {
            return Err(invalid("replanning input identity changed"));
        }
        let config = route
            .plan
            .as_ref()
            .ok_or_else(|| invalid("missing planning config"))?;
        let previous = self.load_plan(host, &context.previous_plan_version_id)?;
        self.validate_frozen_plan(host, &context.previous_plan_version_id, &previous)?;
        if previous.parent_sequence != context.parent_sequence
            || previous.input_version_id != input.version_id
            || previous.proposal_version_id != context.previous_proposal_version_id
            || previous.revision.as_ref().map(|r| r.index + 1) != Some(context.revision)
            || context.context.len() > 128
            || context.steps.len() != previous.steps.len()
        {
            return Err(invalid("replanning context differs from its predecessor"));
        }
        let mut permitted = Vec::new();
        for mut task in self
            .journal
            .plan_step_states(context.parent_sequence, &context.previous_plan_version_id)?
        {
            let step = previous
                .steps
                .iter()
                .find(|s| s.route.id == task.route_id)
                .ok_or_else(|| invalid("unknown previous step"))?;
            let template = export_template(config, step)?;
            task.fields = template.export_fields.clone();
            let snapshot = context
                .steps
                .iter()
                .find(|item| item.sequence == task.sequence)
                .filter(|item| item.id == step.id && item.template_id == step.template_id)
                .ok_or_else(|| invalid("replanning step snapshot identity changed"))?;
            if let Some(version) = &snapshot.failure_receipt_version_id {
                let selection = failure_selection(host, &task, template)?
                    .filter(|item| &item.version_id == version && context.context.contains(item))
                    .filter(|_| snapshot.resolution == "blocked" && snapshot.state == task.state)
                    .ok_or_else(|| {
                        invalid("replanning failure receipt changed or exceeds its export scope")
                    })?;
                permitted.push(selection);
            }
            if let ParentResult::Ready(binding) = self.resolve_parent(host, task)? {
                if snapshot.resolution != "ready"
                    || snapshot.result_version_id.as_ref() != Some(&binding.result_version_id)
                    || !snapshot
                        .output_version_ids
                        .iter()
                        .eq(binding.context.iter().map(|item| &item.version_id))
                {
                    return Err(invalid("replanning step result snapshot changed"));
                }
                permitted.extend(binding.context);
            } else if snapshot.resolution == "ready" {
                return Err(invalid(
                    "completed replanning step no longer has a ready result",
                ));
            }
        }
        if context.context.iter().any(|item| !permitted.contains(item)) {
            return Err(invalid(
                "replanning export is outside the configured step scope",
            ));
        }
        Ok(())
    }

    pub(super) fn replanning_inputs(
        input: &Artifact,
        route: &ReasoningRoute,
        context: &ReplanningContext,
        version: &VersionId,
    ) -> Vec<ContextSelection> {
        let mut inputs = vec![
            ContextSelection {
                version_id: input.version_id.clone(),
                payload_fields: route.payload_fields.clone(),
            },
            ContextSelection {
                version_id: context.previous_proposal_version_id.clone(),
                payload_fields: vec!["steps".into()],
            },
            ContextSelection {
                version_id: version.clone(),
                payload_fields: ["revision", "steps", "omitted_output_items"]
                    .map(str::to_owned)
                    .to_vec(),
            },
        ];
        inputs.extend(context.context.iter().cloned());
        inputs
    }

    pub(super) fn revision_budget(
        &self,
        host: &Host,
        parent: u64,
        config: &PlanningConfig,
        owner: &ReasoningRoute,
        steps: &[AcceptedStep],
    ) -> Result<Option<serde_json::Value>, HostError> {
        let mut budget = super::budgets::PlanBudget::default();
        let history = self.journal.plan_revisions(parent)?;
        for _ in 0..=history.len() {
            budget.add(owner, config.max_total_output_tokens.is_some())?;
        }
        for prior in history {
            let plan = self.load_plan(host, &prior.plan_version_id)?;
            self.validate_frozen_plan(host, &prior.plan_version_id, &plan)?;
            for step in &plan.steps {
                budget.add(&step.route, config.max_total_output_tokens.is_some())?;
            }
        }
        for step in steps {
            budget.add(&step.route, config.max_total_output_tokens.is_some())?;
        }
        if budget.check(config).is_ok() {
            Ok(Some(budget.remaining(config)))
        } else {
            Ok(None)
        }
    }
}

fn export_template<'a>(
    config: &'a PlanningConfig,
    step: &AcceptedStep,
) -> Result<&'a StepTemplate, HostError> {
    config
        .templates
        .iter()
        .find(|template| template.route.id == step.template_id)
        .ok_or_else(|| invalid("missing template export policy"))
}

fn failure_selection(
    host: &Host,
    task: &ParentTask,
    template: &StepTemplate,
) -> Result<Option<ContextSelection>, HostError> {
    let fields = &template.replanning_failure_receipt_fields;
    if fields.is_empty() {
        return Ok(None);
    }
    let goal = template
        .route
        .goal
        .as_ref()
        .ok_or_else(|| invalid("failure receipt needs a goal template"))?;
    failed_goal_receipt(host, task, goal, fields)
}

fn admit_export(context: &mut ReplanningContext, item: ContextSelection, limit: usize) -> bool {
    if context
        .context
        .iter()
        .any(|selected| selected.version_id == item.version_id)
    {
        return false;
    }
    if context.context.len() >= limit {
        context.omitted_output_items += 1;
        return false;
    }
    context.context.push(item);
    true
}

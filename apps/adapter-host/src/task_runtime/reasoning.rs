//! Aufgabenschritte und Reason-Manager verbinden; keine Journal-SQL-Abfragen.
use super::*;
use arcs::reasoning::{ContextSelection, ReasoningRequestV2};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};

impl TaskRuntime {
    pub(super) fn reap_cancelled_task_reasoning(&self, host: &mut Host) -> Result<(), HostError> {
        let cancelled = self
            .journal
            .cancelled_reasoning(&host.pending_reasoning_request_ids())?;
        host.discard_reasoning_requests(&cancelled);
        Ok(())
    }
    pub(super) fn advance_job(
        &mut self,
        host: &mut Host,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
        now_unix_ms: u64,
    ) -> Result<Option<CompletedTask>, HostError> {
        if route.plan.is_some() {
            self.advance_plan(host, sequence, route, input, now_unix_ms)?;
            return Ok(None);
        }
        if let Some(config) = &route.goal {
            self.advance_goal(host, sequence, route, config, input, now_unix_ms)?;
            return Ok(None);
        }
        self.journal.start(sequence)?;
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
            allowed_capabilities: route.allowed_capabilities.clone(),
            constraints: route.constraints.clone(),
            budget: route.budget.clone(),
        };
        let Some(result) =
            self.reason_for_task(host, sequence, request, &[], route.memory.as_ref())?
        else {
            return Ok(None);
        };
        Ok(Some(CompletedTask {
            sequence,
            route_id: route.id.clone(),
            input_version_id: input.version_id.clone(),
            reasoning_status: result.status,
            request_version_id: result.request_version_id,
            result_version_id: result.result_version_id,
            candidate_version_ids: result
                .candidates
                .into_iter()
                .map(|candidate| candidate.version_id)
                .collect(),
        }))
    }

    pub(super) fn record_job_failure(
        &mut self,
        sequence: u64,
        route: &ReasoningRoute,
        error: HostError,
        report: &mut TickReport,
    ) -> Result<(), HostError> {
        if self.config.max_concurrent_reasoning == 1 || is_core_failure(&error) {
            self.journal.fail(sequence, &error)?;
            return Err(error);
        }
        if matches!(error, HostError::AdapterBusy(_)) {
            return Ok(());
        }
        if let Some(mut task) = self.saved_goal(sequence)? {
            task.state = goal::GoalState::Failed;
            task.detail = error.to_string().chars().take(2000).collect();
            self.save_goal(&task)?;
        }
        self.journal.fail(sequence, &error)?;
        report.failed.push(FailedTask {
            sequence,
            route_id: route.id.clone(),
            error: error.to_string(),
        });
        Ok(())
    }

    pub(super) fn reason_for_task(
        &mut self,
        host: &mut Host,
        sequence: u64,
        mut request: ReasoningRequestV2,
        inputs: &[arcs::adapters::input_data::InputDataSelection],
        memory: Option<&arcs::reasoning::context::MemoryPolicy>,
    ) -> Result<Option<arcs::reasoning::general::ReasoningResult>, HostError> {
        let dependencies = self.journal.dependency_bindings(sequence)?;
        if !dependencies.is_empty() {
            request.context.extend(
                dependencies
                    .iter()
                    .flat_map(|binding| binding.context.iter().cloned()),
            );
            let metadata =
                serde_json::to_value(dependencies).map_err(|error| invalid(error.to_string()))?;
            if let Some(object) = request.constraints.as_object_mut() {
                object.insert("task_dependencies".into(), metadata);
            } else {
                request.constraints = serde_json::json!({"operator_constraints":request.constraints,"task_dependencies":metadata});
            }
        }
        let request = match memory {
            Some(policy) => host.recall_context(request, policy, inputs)?,
            None => request,
        };
        self.journal.bind_reasoning(sequence, &request.request_id)?;
        if self.config.max_concurrent_reasoning == 1 {
            host.reason_with_data(request, inputs).map(Some)
        } else {
            host.poll_reason_with_data(request, inputs, self.config.max_concurrent_reasoning)
        }
    }
}

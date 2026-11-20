//! Explicit operator review of a task result before scheduling its delivery.
use arcs::adapters::{AdapterRole, CapabilityContract, CapabilityRef, ProducerClass};
use arcs::approval::{ApprovalDecision, ApprovalError, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SystemClock, VersionId};
use arcs::output::TASK_REPORT_SCHEMA;
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    TaskRuntime, config::invalid, dependencies::ParentResult, is_core_failure,
    output::OutputCommand, plan_output::PlanOutputDraft,
};
use crate::{Host, HostError};

const PREVIEW_LIMIT: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputTarget {
    pub task_sequence: u64,
    pub result_version_id: VersionId,
    pub destination: CapabilityRef,
    pub receipt_schema_id: SchemaId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_output_sequence: Option<u64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputDecision {
    pub target: TaskOutputTarget,
    pub request_version_id: VersionId,
    pub verification_version_id: VersionId,
    pub decision: ApprovalDecision,
    pub operator_id: String,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskOutputCommand {
    Options { task_sequence: u64 },
    Preview { target: TaskOutputTarget },
    PreviewPlanOutput { sequence: u64 },
    Review { review: TaskOutputDecision },
    Inspect { request_version_id: VersionId },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TaskOutputResponse {
    Options(TaskOutputOptions),
    Preview(Box<TaskOutputPreview>),
    Review(TaskOutputReview),
}

#[derive(Debug, Serialize)]
pub struct TaskOutputOptions {
    pub task_sequence: u64,
    pub result_versions: Vec<VersionId>,
    pub destinations: Vec<TaskOutputDestination>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plan_outputs: Vec<PlanOutputDraft>,
}

#[derive(Debug, Serialize)]
pub struct TaskOutputDestination {
    pub destination: CapabilityRef,
    pub receipt_schema_id: SchemaId,
}

#[derive(Debug, Serialize)]
pub struct TaskOutputPreview {
    pub target: TaskOutputTarget,
    pub report_version_id: VersionId,
    pub request_version_id: VersionId,
    pub verification_version_id: VersionId,
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<TaskOutputReview>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutputReviewState {
    Pending,
    Queued,
    Rejected,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct TaskOutputReview {
    pub request_version_id: VersionId,
    pub state: TaskOutputReviewState,
    pub approval_version_id: Option<VersionId>,
    pub delivery_sequence: Option<u64>,
    pub error: Option<String>,
}

impl TaskRuntime {
    pub fn control_task_output(
        &mut self,
        host: &mut Host,
        command: TaskOutputCommand,
    ) -> Result<TaskOutputResponse, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        match command {
            TaskOutputCommand::Options { task_sequence } => self
                .task_output_options(host, task_sequence)
                .map(TaskOutputResponse::Options),
            TaskOutputCommand::Preview { target } => self
                .preview_task_output(host, target)
                .map(Box::new)
                .map(TaskOutputResponse::Preview),
            TaskOutputCommand::PreviewPlanOutput { sequence } => {
                let draft = self.journal.plan_output(sequence)?;
                self.preview_plan_output(host, draft)
                    .map(Box::new)
                    .map(TaskOutputResponse::Preview)
            }
            TaskOutputCommand::Review { review } => self
                .stage_task_output_review(host, review)
                .map(TaskOutputResponse::Review),
            TaskOutputCommand::Inspect { request_version_id } => self
                .journal
                .task_output_review(&request_version_id)?
                .map(|(_, report)| TaskOutputResponse::Review(report))
                .ok_or_else(|| invalid("unknown task output review")),
        }
    }

    fn task_output_options(
        &self,
        host: &Host,
        task_sequence: u64,
    ) -> Result<TaskOutputOptions, HostError> {
        if !(1..=i64::MAX as u64).contains(&task_sequence) {
            return Err(invalid("invalid output source sequence"));
        }
        let plan_outputs = self.journal.plan_outputs(task_sequence)?;
        if !plan_outputs.is_empty() {
            return Ok(TaskOutputOptions {
                task_sequence,
                result_versions: vec![],
                destinations: vec![],
                plan_outputs,
            });
        }
        let result_versions = self.task_output_versions(host, task_sequence)?;
        let mut destinations = Vec::new();
        for destination in host.capabilities(AdapterRole::Output) {
            let (registered, capability) = host
                .registry()
                .authorized_capability(&destination.adapter_id, &destination.capability_id)
                .map_err(HostError::Registration)?;
            if let CapabilityContract::Output {
                accepts,
                emits,
                idempotent: true,
            } = &capability.contract
                && registered.grant().producer_class == ProducerClass::Executor
                && accepts.iter().any(|schema| schema.0 == TASK_REPORT_SCHEMA)
            {
                destinations.extend(emits.iter().map(|schema| TaskOutputDestination {
                    destination: destination.clone(),
                    receipt_schema_id: schema.clone(),
                }));
            }
        }
        let options = TaskOutputOptions {
            task_sequence,
            result_versions,
            destinations,
            plan_outputs,
        };
        if serde_json::to_vec(&options)
            .map_err(output_review_error)?
            .len()
            > PREVIEW_LIMIT
        {
            return Err(invalid(
                "task output options exceed the local preview limit",
            ));
        }
        Ok(options)
    }

    fn preview_task_output(
        &mut self,
        host: &mut Host,
        target: TaskOutputTarget,
    ) -> Result<TaskOutputPreview, HostError> {
        if let Some(sequence) = target.plan_output_sequence {
            let draft = self.journal.plan_output(sequence)?;
            if draft.target()? != target {
                return Err(invalid(
                    "plan output target differs from the configured draft",
                ));
            }
            return self.preview_plan_output(host, draft);
        }
        if !self
            .task_output_versions(host, target.task_sequence)?
            .contains(&target.result_version_id)
        {
            return Err(invalid(
                "output version does not belong to this task's completed result",
            ));
        }
        let report = host.prepare_task_report(
            &target.destination.adapter_id,
            &self.config.runtime_id,
            target.task_sequence,
            &target.result_version_id,
        )?;
        self.finish_task_output_preview(host, target, report)
    }

    pub(super) fn finish_task_output_preview(
        &mut self,
        host: &mut Host,
        target: TaskOutputTarget,
        report: Artifact,
    ) -> Result<TaskOutputPreview, HostError> {
        let request = host.prepare_output(
            &target.destination,
            &report.version_id,
            &target.receipt_schema_id,
        )?;
        // prepare_output checks the installed destination's grants, schemas,
        // exact input and idempotence contract. The operator sees the payload.
        let verification = self.verify_task_output(host, &request)?;
        let review = self
            .journal
            .task_output_review(&request.version_id)?
            .map(|(_, review)| review);
        let preview = TaskOutputPreview {
            target,
            report_version_id: report.version_id,
            request_version_id: request.version_id,
            verification_version_id: verification.version_id,
            payload: report.payload,
            review,
        };
        if serde_json::to_vec(&preview)
            .map_err(output_review_error)?
            .len()
            > PREVIEW_LIMIT
        {
            return Err(invalid("task output exceeds the local preview limit"));
        }
        Ok(preview)
    }

    pub(super) fn task_output_versions(
        &self,
        host: &Host,
        sequence: u64,
    ) -> Result<Vec<VersionId>, HostError> {
        if !(1..=i64::MAX as u64).contains(&sequence) {
            return Err(invalid("invalid output source sequence"));
        }
        if self.journal.saved_plan(sequence)?.is_some() {
            return Err(invalid("select a completed step of this plan"));
        }
        if let Some((parent, generation, _, _)) = self.journal.dynamic_origin(sequence)? {
            let current = self.journal.saved_plan(parent)?.is_some_and(|plan| {
                plan.plan_version.as_ref() == Some(&generation)
                    && matches!(plan.state.as_str(), "accepted" | "ready")
            });
            if !current || self.journal.pending_replan(parent)?.is_some() {
                return Err(invalid("output source step is not in the current plan"));
            }
        }
        let source = self.journal.task_output_source(sequence)?;
        let ParentResult::Ready(binding) = self.resolve_parent(host, source)? else {
            return Err(invalid("task has no completed result available for output"));
        };
        Ok(binding
            .context
            .into_iter()
            .map(|selection| selection.version_id)
            .collect())
    }

    fn verify_task_output(
        &mut self,
        host: &Host,
        request: &Artifact,
    ) -> Result<Artifact, HostError> {
        let stream = format!("verification:{}", request.version_id.0);
        let verifier = OutputContract(request);
        let expected = serde_json::to_value(
            verifier
                .verify(request)
                .map_err(output_review_error)?
                .findings,
        )
        .map_err(output_review_error)?;
        if let Some(saved) = host
            .store()
            .find_by_stream_key(&stream)
            .map_err(HostError::Store)?
        {
            if saved.schema_id.0 != "arcs.verification_report.v1"
                || saved.created_by.actor_type != ActorType::System
                || saved.created_by.id != "arcs.verification"
                || saved.payload["target_version"] != request.version_id.0
                || saved.payload["verdict"] != "pass"
                || saved.payload["findings"] != expected
            {
                return Err(invalid(
                    "saved verification does not match the task output contract",
                ));
            }
            return Ok(saved);
        }
        VerificationService::new(
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .verify(&request.version_id, &verifier)
        .map_err(|error| match error {
            VerificationError::Store(source) => HostError::Store(source),
            error => output_review_error(error),
        })
    }

    fn stage_task_output_review(
        &mut self,
        host: &mut Host,
        review: TaskOutputDecision,
    ) -> Result<TaskOutputReview, HostError> {
        if let Some((prior, status)) = self
            .journal
            .task_output_review(&review.request_version_id)?
        {
            return if prior == review {
                Ok(status)
            } else {
                Err(invalid("an immutable task output decision already exists"))
            };
        }
        if review.operator_id.trim().is_empty()
            || review.operator_id.len() > 128
            || review.operator_id.chars().any(char::is_control)
            || review.reason.trim().is_empty()
            || review.reason.len() > 2000
        {
            return Err(invalid(
                "task output review requires an operator and justification",
            ));
        }
        let preview = self.preview_task_output(host, review.target.clone())?;
        if preview.request_version_id != review.request_version_id
            || preview.verification_version_id != review.verification_version_id
        {
            return Err(invalid(
                "task output decision does not match the previewed request and verification",
            ));
        }
        self.journal.stage_task_output(&review)
    }

    pub(super) fn advance_task_output_reviews(&mut self, host: &mut Host) -> Result<(), HostError> {
        for review in self.journal.pending_task_output_reviews()? {
            let outcome = self.apply_task_output_review(host, &review);
            match outcome {
                Ok((approval, delivery)) => self.journal.finish_task_output_review(
                    &review.request_version_id,
                    Ok((&approval, delivery)),
                )?,
                Err(HostError::OutputQueueFull) => {}
                Err(error) if is_core_failure(&error) => return Err(error),
                Err(error) => self
                    .journal
                    .finish_task_output_review(&review.request_version_id, Err(&error))?,
            }
        }
        Ok(())
    }

    fn apply_task_output_review(
        &mut self,
        host: &mut Host,
        review: &TaskOutputDecision,
    ) -> Result<(VersionId, Option<u64>), HostError> {
        let request = &review.request_version_id;
        let verification = &review.verification_version_id;
        let actor = Actor {
            actor_type: ActorType::Human,
            id: review.operator_id.clone(),
        };
        let stream = format!("approval:{}", request.0);
        let approval = match host
            .store()
            .find_by_stream_key(&stream)
            .map_err(HostError::Store)?
        {
            Some(saved) => {
                if saved.schema_id.0 != "arcs.approval.v1"
                    || saved.created_by != actor
                    || saved.payload["target_version"] != request.0
                    || saved.payload["verification_report_version"] != verification.0
                    || saved.payload["decision"] != json!(review.decision)
                    || saved.payload["reason"] != review.reason
                {
                    return Err(invalid(
                        "saved approval differs from the task output decision",
                    ));
                }
                saved
            }
            None => ApprovalService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .decide(
                request,
                verification,
                review.decision,
                actor,
                review.reason.clone(),
            )
            .map_err(|error| match error {
                ApprovalError::Store(source) => HostError::Store(source),
                error => output_review_error(error),
            })?,
        };
        let delivery = if review.decision == ApprovalDecision::Approved {
            let delivery = self.control_output(
                host,
                OutputCommand::Enqueue {
                    adapter_id: review.target.destination.adapter_id.clone(),
                    request_version_id: request.clone(),
                    approval_version_id: approval.version_id.clone(),
                },
            )?;
            Some(delivery.sequence)
        } else {
            None
        };
        Ok((approval.version_id, delivery))
    }
}

struct OutputContract<'a>(&'a Artifact);

impl Verifier for OutputContract<'_> {
    fn verify(&self, artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        let verdict = if artifact == self.0 {
            VerificationVerdict::Pass
        } else {
            VerificationVerdict::Fail
        };
        Ok(VerificationResult {
            verdict,
            findings: vec![VerificationFinding {
                check: "task_output.bound_contract".into(),
                verdict,
                detail: "Exact output request for the selected task result and authorized adapter contract".into(),
            }],
        })
    }
}

fn output_review_error(error: impl std::fmt::Debug) -> HostError {
    invalid(format!("task output review: {error:?}"))
}

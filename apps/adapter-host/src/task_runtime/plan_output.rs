//! Prepare reports for operator-configured steps of the successful plan generation.
use arcs::{
    adapters::CapabilityRef,
    core::{SchemaId, VersionId},
};
use serde::{Deserialize, Serialize};

use super::{
    TaskRuntime,
    config::invalid,
    is_core_failure,
    task_output::{TaskOutputPreview, TaskOutputTarget},
};
use crate::{Host, HostError};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanOutputSeed {
    pub spec: PlanOutputSource,
    pub export_fields: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanOutputSource {
    pub plan_version_id: VersionId,
    pub task_sequence: u64,
    pub result_version_id: Option<VersionId>,
    pub destination: CapabilityRef,
    pub receipt_schema_id: SchemaId,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOutputState {
    Preparing,
    Ready,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct PlanOutputDraft {
    pub sequence: u64,
    pub parent_sequence: u64,
    pub spec: PlanOutputSource,
    #[serde(skip)]
    pub(super) export_fields: Vec<String>,
    pub state: PlanOutputState,
    pub request_version_id: Option<VersionId>,
    pub verification_version_id: Option<VersionId>,
    pub error: Option<String>,
}

impl PlanOutputDraft {
    pub(super) fn target(&self) -> Result<TaskOutputTarget, HostError> {
        Ok(TaskOutputTarget {
            task_sequence: self.spec.task_sequence,
            result_version_id: self
                .spec
                .result_version_id
                .clone()
                .ok_or_else(|| invalid("plan output result is missing"))?,
            destination: self.spec.destination.clone(),
            receipt_schema_id: self.spec.receipt_schema_id.clone(),
            plan_output_sequence: Some(self.sequence),
        })
    }
}

impl TaskRuntime {
    pub(super) fn advance_plan_outputs(&mut self, host: &mut Host) -> Result<(), HostError> {
        for draft in self.journal.pending_plan_outputs()? {
            let outcome = self.build_plan_output_preview(host, &draft);
            match outcome {
                Ok(preview) => self.journal.finish_plan_output(
                    draft.sequence,
                    Ok((
                        &preview.request_version_id,
                        &preview.verification_version_id,
                    )),
                )?,
                Err(error) if is_core_failure(&error) => return Err(error),
                Err(error) => self
                    .journal
                    .finish_plan_output(draft.sequence, Err(&error))?,
            }
        }
        Ok(())
    }

    pub(super) fn preview_plan_output(
        &mut self,
        host: &mut Host,
        draft: PlanOutputDraft,
    ) -> Result<TaskOutputPreview, HostError> {
        if draft.state != PlanOutputState::Ready {
            return Err(invalid(
                draft
                    .error
                    .as_deref()
                    .unwrap_or("plan output is still preparing"),
            ));
        }
        let preview = self.build_plan_output_preview(host, &draft)?;
        if draft.request_version_id.as_ref() != Some(&preview.request_version_id)
            || draft.verification_version_id.as_ref() != Some(&preview.verification_version_id)
        {
            return Err(invalid("saved plan output proof changed"));
        }
        Ok(preview)
    }

    fn build_plan_output_preview(
        &mut self,
        host: &mut Host,
        draft: &PlanOutputDraft,
    ) -> Result<TaskOutputPreview, HostError> {
        let target = draft.target()?;
        let parent = self
            .journal
            .saved_plan(draft.parent_sequence)?
            .ok_or_else(|| invalid("plan output has no saved plan"))?;
        let origin = self.journal.dynamic_origin(target.task_sequence)?;
        if parent.state != "ready"
            || parent.plan_version.as_ref() != Some(&draft.spec.plan_version_id)
            || !origin.is_some_and(|(sequence, version, _, _)| {
                sequence == draft.parent_sequence && version == draft.spec.plan_version_id
            })
            || !self
                .task_output_versions(host, target.task_sequence)?
                .contains(&target.result_version_id)
        {
            return Err(invalid(
                "plan output does not belong to the completed plan generation",
            ));
        }
        let report = host.prepare_plan_report(
            &target.destination.adapter_id,
            &self.config.runtime_id,
            target.task_sequence,
            &target.result_version_id,
            &draft.spec.plan_version_id,
            &draft.export_fields,
        )?;
        self.finish_task_output_preview(host, target, report)
    }
}

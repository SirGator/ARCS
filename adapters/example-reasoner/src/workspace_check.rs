//! The example plans a named local check and interprets its current receipt.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::{Value, json};

pub const CANDIDATE: &str = "arcs.workspace_check_candidate.v1";
use crate::GOAL_ASSESSMENT as ASSESSMENT;
const TASK: &str = "arcs.task.workspace_check.v1";
const EXECUTION: &str = "arcs.execution_result.workspace_check.v1";

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    let metadata = &invocation.constraints["goal_runtime"];
    let task_id = &metadata["task_version_id"];
    let task = invocation
        .context
        .iter()
        .find(|item| {
            item.schema_id.0 == TASK && task_id.as_str() == Some(item.version_id.0.as_str())
        })
        .ok_or_else(|| AdapterError::rejected("missing bound workspace check task"))?;
    let check = task.payload["check"]
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AdapterError::rejected("workspace check needs a name"))?;
    let (payload, required_capabilities, referenced_versions) = match (
        invocation.target_schema_id.0.as_str(),
        metadata["phase"].as_str(),
    ) {
        (CANDIDATE, Some("plan")) => {
            let [capability] = invocation.allowed_capabilities.as_slice() else {
                return Err(AdapterError::rejected(
                    "workspace planner requires one configured check capability",
                ));
            };
            if capability.capability_id.0 != "check.run" {
                return Err(AdapterError::rejected(
                    "workspace planner requires check.run",
                ));
            }
            (
                json!({"check":check}),
                vec![capability.clone()],
                vec![task.version_id.clone()],
            )
        }
        (ASSESSMENT, Some("assess")) => {
            let execution_id = &metadata["execution_version_id"];
            let execution = invocation
                .context
                .iter()
                .find(|item| {
                    item.schema_id.0 == EXECUTION
                        && execution_id.as_str() == Some(item.version_id.0.as_str())
                })
                .ok_or_else(|| AdapterError::rejected("missing bound workspace check receipt"))?;
            let result = &execution.payload["result"];
            if result["check"] != check {
                return Err(AdapterError::rejected(
                    "receipt belongs to another workspace check",
                ));
            }
            let (goal, effect, detail) = match result["status"].as_str() {
                Some("passed") if result["exit_code"].as_i64() == Some(0) => {
                    ("achieved", "success", "the named project check passed")
                }
                Some("failed") => (
                    "continue",
                    "failure",
                    "the project check failed and can be repeated after a correction",
                ),
                _ => (
                    "blocked",
                    "unknown",
                    "the receipt does not prove a completed project check",
                ),
            };
            (
                json!({
                    "goal_status":goal,"effect":effect,"detail":detail,
                    "evidence_version_ids":[execution.version_id]
                }),
                vec![],
                vec![task.version_id.clone(), execution.version_id.clone()],
            )
        }
        _ => return Err(AdapterError::rejected("unsupported workspace goal phase")),
    };
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status: ReasoningStatus::Solved,
        candidates: vec![ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities,
            referenced_versions,
            payload,
        }],
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "workspace-check-rules".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details: Value::Null,
        },
    })
}

//! A rules-based repair plan hands a verified Edit receipt to its check step.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningContextItem, ReasoningInvocationV2, ReasoningResponseV2,
    ReasoningStatus, ReasoningTraceV2, VersionId,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::{Value, json};

use crate::GOAL_ASSESSMENT;

pub const PLAN: &str = "arcs.plan_candidate.v1";
pub const EDIT: &str = "arcs.workspace_edit_candidate.v1";
pub const CHECK: &str = "arcs.workspace_revision_check_candidate.v1";
const TASK: &str = "arcs.task.workspace_repair.v1";
const EDIT_RECEIPT: &str = "arcs.execution_result.workspace_edit.v1";
const CHECK_RECEIPT: &str = "arcs.execution_result.workspace_revision_check.v1";

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    let metadata = &invocation.constraints["goal_runtime"];
    let target = invocation.target_schema_id.0.as_str();
    let task = invocation
        .context
        .iter()
        .find(|item| {
            item.schema_id.0 == TASK
                && (target == PLAN || identifies(&metadata["task_version_id"], &item.version_id))
        })
        .ok_or_else(|| AdapterError::rejected("missing workspace repair task"))?;
    let mut versions = vec![task.version_id.clone()];
    let mut required_capabilities = Vec::new();
    let payload = match (target, metadata["phase"].as_str()) {
        (PLAN, None) if invocation.constraints.get("task_planning").is_some() => json!({
            "steps":[
                {"id":"edit","template":"edit"},
                {"id":"check","template":"check","dependencies":[{"step_id":"edit","payload_fields":["result"]}]}
            ]
        }),
        (EDIT, Some("plan")) => {
            required_capabilities.push(select_action(invocation, "edit.apply")?);
            json!({"base_commit":task.payload["base_commit"],"path":task.payload["path"],
                "replacement":task.payload["replacement"]})
        }
        (CHECK, Some("plan")) => {
            required_capabilities.push(select_action(invocation, "check.revision")?);
            let [binding] = invocation.constraints["task_dependencies"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
            else {
                return Err(AdapterError::rejected(
                    "revision check requires an edit dependency",
                ));
            };
            let [source] = binding["context"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
            else {
                return Err(AdapterError::rejected(
                    "revision check requires one bound edit receipt",
                ));
            };
            let receipt = bound_context(invocation, &source["version_id"], EDIT_RECEIPT)?;
            let revision =
                successful_edit(&receipt.payload["result"], &task.payload).ok_or_else(|| {
                    AdapterError::rejected("edit receipt does not prove the requested repair")
                })?;
            versions.push(receipt.version_id.clone());
            json!({"check":task.payload["check"],"revision":revision})
        }
        (GOAL_ASSESSMENT, Some("assess")) => {
            let candidate_id = &metadata["candidate_version_id"];
            let candidate = invocation
                .context
                .iter()
                .find(|item| identifies(candidate_id, &item.version_id))
                .ok_or_else(|| AdapterError::rejected("missing workspace repair candidate"))?;
            let schema = match candidate.schema_id.0.as_str() {
                EDIT => EDIT_RECEIPT,
                CHECK => CHECK_RECEIPT,
                _ => {
                    return Err(AdapterError::rejected(
                        "unsupported workspace repair candidate",
                    ));
                }
            };
            let receipt = bound_context(invocation, &metadata["execution_version_id"], schema)?;
            let result = &receipt.payload["result"];
            let (success, detail) = if candidate.schema_id.0 == EDIT {
                (
                    successful_edit(result, &candidate.payload).is_some(),
                    "The edit receipt must confirm publication of the requested file change.",
                )
            } else {
                (
                    result["status"] == "passed"
                        && result["exit_code"] == 0
                        && result["revision"] == candidate.payload["revision"]
                        && result["check"] == candidate.payload["check"],
                    "The named check must pass on the requested Git commit.",
                )
            };
            versions.extend([candidate.version_id.clone(), receipt.version_id.clone()]);
            json!({
                "goal_status": if success {"achieved"} else {"blocked"},
                "effect": if success {"success"} else {"unknown"},
                "detail":detail,
                "evidence_version_ids":[receipt.version_id]
            })
        }
        _ => return Err(AdapterError::rejected("unsupported workspace repair phase")),
    };
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status: ReasoningStatus::Solved,
        candidates: vec![ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities,
            referenced_versions: versions,
            payload,
        }],
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "workspace-repair-rules".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details: Value::Null,
        },
    })
}

fn identifies(value: &Value, version: &VersionId) -> bool {
    value.as_str() == Some(version.0.as_str())
}

fn bound_context<'a>(
    invocation: &'a ReasoningInvocationV2,
    version: &Value,
    schema: &str,
) -> Result<&'a ReasoningContextItem, AdapterError> {
    invocation
        .context
        .iter()
        .find(|item| identifies(version, &item.version_id) && item.schema_id.0 == schema)
        .ok_or_else(|| AdapterError::rejected("missing bound workspace repair receipt"))
}

fn select_action(
    invocation: &ReasoningInvocationV2,
    id: &str,
) -> Result<arcs_adapter_protocol::CapabilityRef, AdapterError> {
    let [capability] = invocation.allowed_capabilities.as_slice() else {
        return Err(AdapterError::rejected(
            "workspace repair needs one configured action",
        ));
    };
    if capability.capability_id.0 != id {
        return Err(AdapterError::rejected("unexpected workspace repair action"));
    }
    Ok(capability.clone())
}

fn successful_edit<'a>(result: &'a Value, change: &Value) -> Option<&'a str> {
    if !matches!(result["status"].as_str(), Some("applied" | "unchanged"))
        || result["path"] != change["path"]
        || result["base_commit"] != change["base_commit"]
    {
        return None;
    }
    result["commit"].as_str()
}

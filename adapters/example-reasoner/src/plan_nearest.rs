//! Beispielplanung außerhalb des Core: ein oder drei Schritte je nach Eingabe.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::json;
pub const CANDIDATE: &str = "arcs.plan_candidate.v1";

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    let input = invocation
        .context
        .iter()
        .find(|item| item.schema_id.0 == super::TASK)
        .ok_or_else(|| AdapterError::rejected("missing planning task"))?;
    let task: super::Task = serde_json::from_value(input.payload.clone())
        .map_err(|e| AdapterError::rejected(e.to_string()))?;
    if invocation.target_schema_id.0 != CANDIDATE {
        return Err(AdapterError::rejected("unsupported plan target"));
    }
    let catalog = invocation.constraints["task_planning"]["templates"]
        .as_array()
        .ok_or_else(|| AdapterError::rejected("missing authorized planning catalog"))?;
    let required = if task.values.len() > 1 {
        vec!["solver", "model", "sum"]
    } else {
        vec!["solver"]
    };
    if required.iter().any(|id| {
        !catalog
            .iter()
            .any(|template| template["id"].as_str() == Some(id))
    }) {
        return Err(AdapterError::rejected(
            "required example template is not authorized",
        ));
    }
    let steps = if task.values.len() > 1 {
        json!([{"id":"exact","template":"solver"},{"id":"alternative","template":"model"},{"id":"combined","template":"sum","dependencies":[{"step_id":"exact","payload_fields":["value"]},{"step_id":"alternative","payload_fields":["value"]}]}])
    } else {
        json!([{"id":"single","template":"solver"}])
    };
    let status = if task.values.is_empty() {
        ReasoningStatus::NoSolution
    } else {
        ReasoningStatus::Solved
    };
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status,
        candidates: if status == ReasoningStatus::Solved {
            vec![ProposalSubmission {
                schema_id: invocation.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: vec![input.version_id.clone()],
                payload: json!({"steps":steps}),
            }]
        } else {
            vec![]
        },
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "nearest-workflow-planner".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details: json!({"example":true}),
        },
    })
}

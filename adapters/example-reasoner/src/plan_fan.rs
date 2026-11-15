//! Rules demonstration: select an operator-authorized, manually approved goal.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::json;

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    if invocation.target_schema_id.0 != super::plan_nearest::CANDIDATE {
        return Err(AdapterError::rejected("unsupported plan target"));
    }
    let input = invocation
        .context
        .iter()
        .find(|item| item.schema_id.0 == "arcs.task.fan_goal.v1")
        .ok_or_else(|| AdapterError::rejected("missing fan goal task"))?;
    let catalog = invocation.constraints["task_planning"]["templates"]
        .as_array()
        .ok_or_else(|| AdapterError::rejected("missing authorized planning catalog"))?;
    let template = catalog.iter().find(|template| {
        template["kind"] == "goal"
            && template["target_schema_id"] == super::goal_fan::CANDIDATE
            && template["goal_limits"]["approval"] == "manual"
    });
    let candidates = template
        .map(|template| ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities: vec![],
            referenced_versions: vec![input.version_id.clone()],
            payload: json!({"steps":[{"id":"device","template":template["id"]}]}),
        })
        .into_iter()
        .collect::<Vec<_>>();
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status: if candidates.is_empty() {
            ReasoningStatus::NoSolution
        } else {
            ReasoningStatus::Solved
        },
        candidates,
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "fan-workflow-planner".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details: json!({"example":true}),
        },
    })
}

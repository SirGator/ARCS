//! Fachliches Rules-Beispiel außerhalb von Core und allgemeinem Host.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde::Deserialize;
use serde_json::{Value, json};

pub const CANDIDATE: &str = "arcs.fan_candidate.v1";
pub const ASSESSMENT: &str = "arcs.goal_assessment_candidate.v1";
const TASK: &str = "arcs.task.fan_goal.v1";
const STATE: &str = "arcs.state.fan.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    device_id: String,
    enabled: bool,
    minimum_actuations: u64,
}
fn rejected(message: impl std::fmt::Display) -> AdapterError {
    AdapterError::rejected(message.to_string())
}

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    let task: Task = serde_json::from_value(
        invocation
            .context
            .iter()
            .find(|item| item.schema_id.0 == TASK)
            .ok_or_else(|| rejected("missing fan goal"))?
            .payload
            .clone(),
    )
    .map_err(rejected)?;
    let states: Vec<_> = invocation
        .context
        .iter()
        .filter(|item| item.schema_id.0 == STATE && item.payload["device_id"] == task.device_id)
        .collect();
    let metadata = &invocation.constraints["goal_runtime"];
    let phase = if invocation.target_schema_id.0 == ASSESSMENT {
        "after"
    } else {
        "before"
    };
    let selected = metadata[phase]["fan-state"]
        .as_str()
        .ok_or_else(|| rejected("missing source version binding"))?;
    let state = states
        .iter()
        .find(|item| item.version_id.0 == selected)
        .ok_or_else(|| rejected("missing bound fan state"))?;
    let enabled = state.payload["enabled"]
        .as_bool()
        .ok_or_else(|| rejected("invalid enabled state"))?;
    let count = state.payload["actuation_count"]
        .as_u64()
        .ok_or_else(|| rejected("invalid actuation count"))?;
    let (payload, required_capabilities) = match invocation.target_schema_id.0.as_str() {
        CANDIDATE => {
            let capability = invocation
                .allowed_capabilities
                .first()
                .ok_or_else(|| rejected("no allowed fan capability"))?;
            (
                json!({"device_id":task.device_id,"enabled":!enabled}),
                vec![capability.clone()],
            )
        }
        ASSESSMENT => {
            let execution = invocation
                .context
                .iter()
                .find(|item| item.artifact_type == "execution_result")
                .ok_or_else(|| rejected("missing execution receipt"))?;
            let command = invocation
                .context
                .iter()
                .find(|item| item.schema_id.0 == CANDIDATE)
                .ok_or_else(|| rejected("missing command"))?;
            let before_id = metadata["before"]["fan-state"]
                .as_str()
                .ok_or_else(|| rejected("missing before source binding"))?;
            let before = states
                .iter()
                .find(|item| item.version_id.0 == before_id)
                .ok_or_else(|| rejected("missing before state"))?;
            let effect = execution.payload["result"] == state.payload
                && command.payload["enabled"] == enabled
                && before.payload["actuation_count"]
                    .as_u64()
                    .and_then(|n| n.checked_add(1))
                    == Some(count);
            (
                json!({"goal_status":if effect && count >= task.minimum_actuations && enabled == task.enabled {"achieved"} else {"continue"},
                "effect":if effect {"success"} else {"failure"},"detail":"compare separate before/after state, execution receipt and requested final state/count",
                "evidence_version_ids":states.iter().map(|state| state.version_id.clone()).collect::<Vec<_>>()}),
                vec![],
            )
        }
        _ => return Err(rejected("unsupported fan goal result schema")),
    };
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status: ReasoningStatus::Solved,
        candidates: vec![ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities,
            referenced_versions: invocation
                .context
                .iter()
                .map(|item| item.version_id.clone())
                .collect(),
            payload,
        }],
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "fan-cycle-rules".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details: Value::Null,
        },
    })
}

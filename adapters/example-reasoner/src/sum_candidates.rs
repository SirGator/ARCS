//! Deterministisches Beispiel für die Zusammenführung externer Vorschläge.
//! Rechenwissen bleibt im Adapter; der Core bindet nur erlaubten Kontext.
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::json;

pub const CANDIDATE: &str = "arcs.solution_candidate.sum.v1";

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    if invocation.target_schema_id.0 != CANDIDATE {
        return Err(AdapterError::rejected("unsupported sum result schema"));
    }
    let bindings = invocation.constraints["task_dependencies"]
        .as_array()
        .ok_or_else(|| AdapterError::rejected("missing named dependency bindings"))?;
    let mut inputs = Vec::new();
    for binding in bindings {
        for selection in binding["context"]
            .as_array()
            .ok_or_else(|| AdapterError::rejected("invalid dependency selection"))?
        {
            let version = selection["version_id"]
                .as_str()
                .ok_or_else(|| AdapterError::rejected("missing dependency version"))?;
            let item = invocation
                .context
                .iter()
                .find(|item| {
                    item.version_id.0 == version
                        && item.schema_id.0 == "arcs.solution_candidate.nearest.v1"
                })
                .ok_or_else(|| AdapterError::rejected("missing bound candidate context"))?;
            inputs.push(item);
        }
    }
    if inputs.is_empty() || inputs.len() > 32 {
        return Err(AdapterError::rejected("sum needs 1..32 candidate inputs"));
    }
    let mut sum = 0.0;
    for item in &inputs {
        let value = item.payload["value"]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| AdapterError::rejected("missing finite candidate value"))?;
        sum += value;
        if !sum.is_finite() {
            return Err(AdapterError::rejected("candidate sum overflow"));
        }
    }
    Ok(ReasoningResponseV2 {
        invocation_id: invocation.invocation_id.clone(),
        request_id: invocation.request_id.clone(),
        status: ReasoningStatus::Solved,
        candidates: vec![ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities: vec![],
            referenced_versions: inputs.iter().map(|item| item.version_id.clone()).collect(),
            payload: json!({"sum":sum,"count":inputs.len()}),
        }],
        trace: ReasoningTraceV2 {
            method: invocation.method,
            implementation: "sum-candidate-values".into(),
            version: "1".into(),
            iterations: Some(inputs.len() as u64),
            output_tokens: None,
            details: json!({}),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_memory_with_the_same_schema_cannot_replace_a_named_dependency() {
        let context:Vec<_>=[("left",2),("right",1),("remembered-other-task",999)].into_iter()
            .map(|(id,value)|json!({"version_id":id,"schema_id":"arcs.solution_candidate.nearest.v1","artifact_type":"candidate","trust_level":"low","payload":{"value":value}})).collect();
        let mut invocation:ReasoningInvocationV2=serde_json::from_value(json!({
            "invocation_id":"sum-1","request_id":"request-1","capability":{"adapter_id":"sum","capability_id":"solution.propose"},
            "method":"rules","objective":"sum the two named results","context":context,"target_schema_id":CANDIDATE,"allowed_capabilities":[],
            "constraints":{"task_dependencies":[{"route_id":"left-task","context":[{"version_id":"left"}]},{"route_id":"right-task","context":[{"version_id":"right"}]}]},
            "budget":{"max_context_items":3,"max_context_bytes":8192,"max_output_bytes":4096,"max_candidates":1,"max_runtime_ms":1000,"max_iterations":32,"max_output_tokens":null}
        })).unwrap();
        let response = reason(&invocation).unwrap();
        assert_eq!(response.candidates[0].payload, json!({"sum":3.0,"count":2}));
        assert_eq!(
            response.candidates[0]
                .referenced_versions
                .iter()
                .map(|id| id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["left", "right"]
        );
        invocation
            .context
            .retain(|item| item.version_id.0 != "right");
        assert!(reason(&invocation).is_err());
    }
}

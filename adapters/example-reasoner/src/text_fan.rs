//! Begrenzter natürlicher Texteingang als externer Rules-Reasoner. Keine
//! Textinterpretation im Core, Host oder GUI; kein allgemeines Sprachmodell.
use super::goal_fan::{ASSESSMENT, CANDIDATE};
use arcs_adapter_protocol::{
    ProposalSubmission, ReasoningInvocationV2, ReasoningResponseV2, ReasoningStatus,
    ReasoningTraceV2,
};
use arcs_adapter_sdk::AdapterError;
use serde_json::{Value, json};

fn command(text: &str) -> Option<Vec<bool>> {
    let normalized = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_lowercase()
        .replace("luefter", "lüfter");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = normalized.strip_prefix("bitte ").unwrap_or(&normalized);
    match normalized {
        "schalte den lüfter ein"
        | "schalte den lüfter an"
        | "schalte lüfter ein"
        | "lüfter einschalten"
        | "lüfter an"
        | "turn the fan on" => Some(vec![true]),
        "schalte den lüfter aus"
        | "schalte lüfter aus"
        | "lüfter ausschalten"
        | "lüfter aus"
        | "turn the fan off" => Some(vec![false]),
        "schalte den lüfter ein und dann wieder aus" | "schalte den lüfter ein und danach aus" => {
            Some(vec![true, false])
        }
        "schalte den lüfter aus und dann wieder ein" | "schalte den lüfter aus und danach ein" => {
            Some(vec![false, true])
        }
        _ => None,
    }
}
fn rejected(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::rejected(error.to_string())
}

pub fn reason(invocation: &ReasoningInvocationV2) -> Result<ReasoningResponseV2, AdapterError> {
    let text = invocation
        .context
        .iter()
        .find(|item| item.schema_id.0 == "arcs.input.v1")
        .and_then(|item| item.payload["raw_text"].as_str())
        .ok_or_else(|| rejected("missing text task"))?;
    let Some(steps) = command(text) else {
        return Ok(response(
            invocation,
            vec![],
            json!({"explanation":"Der angeschlossene Beispiel-Reasoner versteht diese Aufgabe nicht. Unterstützt sind zum Beispiel: Schalte den Lüfter ein; Schalte den Lüfter aus; Schalte den Lüfter ein und dann wieder aus."}),
        ));
    };
    let metadata = &invocation.constraints["goal_runtime"];
    let initial = metadata["phase"] == "check";
    let after = metadata["phase"] == "assess";
    let round = metadata["round"]
        .as_u64()
        .ok_or_else(|| rejected("missing goal round"))? as usize;
    let selected = metadata[if after { "after" } else { "before" }]["fan-state"]
        .as_str()
        .ok_or_else(|| rejected("missing bound state"))?;
    let state = invocation
        .context
        .iter()
        .find(|item| item.version_id.0 == selected && item.schema_id.0 == "arcs.state.fan.v1")
        .ok_or_else(|| rejected("missing fan state"))?;
    let enabled = state.payload["enabled"]
        .as_bool()
        .ok_or_else(|| rejected("invalid fan state"))?;
    let device = state.payload["device_id"]
        .as_str()
        .ok_or_else(|| rejected("missing device"))?;
    let step = *steps
        .get(round.saturating_sub(1))
        .ok_or_else(|| rejected("text plan round exceeded"))?;
    let (payload, required_capabilities) = if invocation.target_schema_id.0 == CANDIDATE {
        let capability = invocation
            .allowed_capabilities
            .iter()
            .find(|capability| capability.capability_id.0 == "fan.set")
            .ok_or_else(|| rejected("no permitted fan action"))?;
        (
            json!({"device_id":device,"enabled":step}),
            vec![capability.clone()],
        )
    } else if invocation.target_schema_id.0 == ASSESSMENT {
        let (goal, effect, detail) = if initial {
            if steps.len() == 1 && enabled == step {
                (
                    "achieved",
                    "unknown",
                    "Das gewünschte Ziel ist bereits erreicht. Es war keine Aktion nötig.",
                )
            } else {
                (
                    "continue",
                    "unknown",
                    "Aufgabe verstanden. Ein Handlungsvorschlag wird erstellt.",
                )
            }
        } else {
            let before_id = metadata["before"]["fan-state"]
                .as_str()
                .ok_or_else(|| rejected("missing before state reference"))?;
            let before = invocation
                .context
                .iter()
                .find(|item| item.version_id.0 == before_id)
                .ok_or_else(|| rejected("missing before state"))?;
            let execution_id = metadata["execution_version_id"]
                .as_str()
                .ok_or_else(|| rejected("missing execution reference"))?;
            let execution = invocation
                .context
                .iter()
                .find(|item| item.version_id.0 == execution_id)
                .ok_or_else(|| rejected("missing execution"))?;
            let delta = u64::from(before.payload["enabled"].as_bool() != Some(enabled));
            let observed = enabled == step
                && execution.payload["result"] == state.payload
                && before.payload["actuation_count"]
                    .as_u64()
                    .and_then(|count| count.checked_add(delta))
                    == state.payload["actuation_count"].as_u64();
            if !observed {
                (
                    "blocked",
                    "failure",
                    "Die beobachtete Wirkung bestätigt den gewünschten Schritt nicht.",
                )
            } else if round < steps.len() {
                (
                    "continue",
                    "success",
                    "Der erste Schritt ist bestätigt. Der nächste Schritt wird geplant.",
                )
            } else {
                (
                    "achieved",
                    "success",
                    if enabled {
                        "Der Lüfter ist eingeschaltet."
                    } else {
                        "Der Lüfter ist ausgeschaltet."
                    },
                )
            }
        };
        let evidence = metadata[if after { "after" } else { "before" }]
            .as_object()
            .ok_or_else(|| rejected("missing source references"))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        (
            json!({"goal_status":goal,"effect":effect,"detail":detail,"evidence_version_ids":evidence}),
            vec![],
        )
    } else {
        return Err(rejected("unsupported text fan output schema"));
    };
    Ok(response(
        invocation,
        vec![ProposalSubmission {
            schema_id: invocation.target_schema_id.clone(),
            required_capabilities,
            referenced_versions: invocation
                .context
                .iter()
                .map(|item| item.version_id.clone())
                .collect(),
            payload,
        }],
        Value::Null,
    ))
}

fn response(
    invocation: &ReasoningInvocationV2,
    candidates: Vec<ProposalSubmission>,
    details: Value,
) -> ReasoningResponseV2 {
    ReasoningResponseV2 {
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
            implementation: "bounded-text-fan-rules".into(),
            version: "1".into(),
            iterations: Some(1),
            output_tokens: None,
            details,
        },
    }
}

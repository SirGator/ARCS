use arcs::adapters::CapabilityRef;
use arcs::core::SchemaId;
use arcs::reasoning::{ContextSelection, ReasoningMethod, ReasoningRequestV2, ReasoningResources};
use arcs_host::Host;
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: reason-task <host.json> <reasoner-adapter-id> <solver|learned>".into());
    }
    let adapter = args[1].to_str().ok_or("adapter ID must be UTF-8")?;
    let method: ReasoningMethod = serde_json::from_value(serde_json::json!(
        args[2].to_str().ok_or("method must be UTF-8")?
    ))?;
    let mut host = Host::open(std::path::Path::new(&args[0]))?;
    host.poll_inputs()?;
    let input = host
        .store()
        .committed_after(0)
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .map(|event| event.artifact)
        .find(|artifact| {
            artifact.schema_id.0 == "arcs.task.nearest.v1"
                && artifact.created_by.id == "example.input"
        })
        .ok_or("no saved task input")?;
    let result = host.reason(ReasoningRequestV2 {
        request_id: format!("nearest:{}", input.version_id.0),
        capability: CapabilityRef::new(adapter, "solution.propose"),
        method,
        objective: "Select a listed value with minimum absolute distance to target".into(),
        context: vec![ContextSelection {
            version_id: input.version_id,
            payload_fields: vec!["values".into(), "target".into()],
        }],
        target_schema_id: SchemaId("arcs.solution_candidate.nearest.v1".into()),
        allowed_capabilities: vec![],
        constraints: serde_json::json!({}),
        budget: ReasoningResources {
            max_context_items: 1,
            max_context_bytes: 8192,
            max_output_bytes: 4096,
            max_candidates: 1,
            max_runtime_ms: 1000,
            max_iterations: if method == ReasoningMethod::Solver {
                Some(4096)
            } else {
                None
            },
            max_output_tokens: None,
        },
    })?;
    host.shutdown()?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("reason task: {error}");
        std::process::exit(1);
    }
}

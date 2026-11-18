//! Explicit sample operator reviews followed by normal bounded task execution.
use arcs::core::{Actor, ActorType};
use arcs::memory::{MemoryAssessmentCommand, MemoryStatus};
use arcs::reasoning::attempts::AttemptQuery;
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use serde_json::json;
use std::{path::Path, time::Duration};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: memory-assessments <host.json> <runtime.json>".into());
    }
    let mut host = Host::open(Path::new(&args[0]))?;
    host.poll_inputs()?;
    // This isolated example has four notes and a single task, not an unbounded
    // operator search over an arbitrary production history.
    let initial = host
        .store()
        .committed_page_after(0, 256)
        .map_err(|e| format!("{e:?}"))?;
    if initial.len() == 256 {
        return Err("example history exceeds its fixed 256-row budget".into());
    }
    let mut assessments = Vec::new();
    for (reference, status) in [
        ("memory:active", MemoryStatus::Active),
        ("memory:retired", MemoryStatus::Retracted),
    ] {
        let note = initial
            .iter()
            .map(|e| &e.artifact)
            .find(|a| {
                a.schema_id.0 == "arcs.note.project_memory.v1" && a.source.reference == reference
            })
            .ok_or("example note missing")?;
        assessments.push(host.assess_memory(&MemoryAssessmentCommand {
            target_version_id: note.version_id.clone(),
            expected_version_id: None,
            decision_by: Actor {
                actor_type: ActorType::Human,
                id: "example-operator".into(),
            },
            reason: "Explicit sample operator assessment for the demonstration".into(),
            status,
            valid_from: None,
            valid_until: None,
            evidence: vec![],
            replacement_version_id: None,
        })?);
    }
    let mut runtime = TaskRuntime::open(&host, RuntimeConfig::load(Path::new(&args[1]))?)?;
    let mut ticks = 0;
    while ticks < 50 && runtime.report(ticks)?.ready_for_review == 0 {
        runtime.tick(&mut host)?;
        ticks += 1;
        std::thread::sleep(Duration::from_millis(20));
    }
    let report = runtime.report(ticks)?;
    if report.ready_for_review != 1 {
        return Err("example task did not reach review".into());
    }
    let history = host.reasoning_attempts(&AttemptQuery {
        capabilities: vec![arcs::adapters::CapabilityRef::new(
            "example.solver",
            "solution.propose",
        )],
        after_sequence: 0,
        limit: 16,
    })?;
    let selection = host
        .store()
        .committed_page_after(0, 256)
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .map(|e| e.artifact)
        .find(|a| a.schema_id.0 == "arcs.context_selection.v2")
        .ok_or("missing context evidence")?;
    host.shutdown()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"runtime":report,"history":history,"assessments":assessments,"context_selection":selection})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("memory assessments: {error}");
        std::process::exit(1);
    }
}

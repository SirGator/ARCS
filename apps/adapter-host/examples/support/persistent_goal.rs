//! Application-level qualification of the normal Host APIs. Model inference
//! stays in the configured external adapter; this module never invents answers.
#![allow(dead_code)]
use arcs::adapters::AdapterGrant;
use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, Artifact, VersionId};
use arcs::reasoning::attempts::{AttemptOutcome, AttemptQuery};
use arcs::reasoning::{ReasoningInvocationV2, ReasoningStatus};
use arcs_host::Host;
use arcs_host::config::HostConfig;
use arcs_host::task_runtime::goal::{GoalApproval, GoalState, GoalTask};
use arcs_host::task_runtime::scheduler::Activation;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Options {
    pub host: PathBuf,
    pub runtime: PathBuf,
    /// Supplied by the test application, never by a model or profile claim.
    pub simulator_program: PathBuf,
    pub start_unix_ms: u64,
    pub phase_timeout: Duration,
}
#[derive(Default, Serialize)]
pub struct Evidence {
    pub passed: bool,
    pub core_database: Option<PathBuf>,
    pub error: Option<String>,
    pub start_unix_ms: u64,
    pub simulated_delay_ms: u64,
    pub wall_elapsed_ms: u128,
    pub checkpoints: Vec<Value>,
    pub attempt_versions: Vec<VersionId>,
    pub adapter_reported_models: Vec<Value>,
}
fn check(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    check(
        bytes.len() <= 65536,
        "qualification configuration exceeds 65536 bytes",
    )?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn resolve(base: &Path, path: &Path) -> PathBuf {
    base.join(path)
}

/// Check the action executable before opening any Core or starting children.
/// The explicit approval flag in the CLI only applies to this known simulator.
pub fn validate(options: &Options) -> Result<(RuntimeConfig, AttemptQuery, u64), String> {
    check(
        (1..=900).contains(&options.phase_timeout.as_secs()),
        "phase timeout must be 1..900 seconds",
    )?;
    let runtime = RuntimeConfig::load(&options.runtime).map_err(|e| e.to_string())?;
    check(
        runtime.routes.len() == 1,
        "qualification requires exactly one goal route",
    )?;
    let route = &runtime.routes[0];
    let goal = route.goal.as_ref().ok_or("qualification requires a goal")?;
    check(
        matches!(goal.approval, GoalApproval::Manual)
            && goal.max_rounds == 2
            && goal.initial_assessment,
        "qualification requires two rounds, initial checks and manual approval",
    )?;
    check(
        goal.actions.len() == 1 && goal.actions[0].capability.capability_id.0 == "fan.set",
        "qualification only approves a single simulated fan capability",
    )?;
    check(
        route.memory.is_some(),
        "qualification requires an explicit memory policy",
    )?;
    let delay = match goal.between_rounds {
        Some(Activation::AfterDelay { delay_ms }) if delay_ms > 0 => delay_ms,
        _ => return Err("qualification requires a persistent delay between rounds".into()),
    };
    let host: HostConfig = read(&options.host)?;
    let base = options
        .host
        .parent()
        .ok_or("missing configuration directory")?;
    let expected = options
        .simulator_program
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut found = false;
    for adapter in &host.adapters {
        let grant: AdapterGrant = read(&resolve(base, &adapter.grant_file))?;
        if grant.adapter_id == goal.actions[0].capability.adapter_id {
            check(
                resolve(base, &adapter.program)
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    == expected,
                "qualification refuses automatic review of a different actuator executable",
            )?;
            found = true;
        }
    }
    check(found, "simulator installation is missing")?;
    let query = AttemptQuery {
        capabilities: vec![route.reasoner.clone(), goal.assessment.capability.clone()],
        after_sequence: 0,
        limit: 32,
    };
    query.validate().map_err(|e| format!("{e:?}"))?;
    Ok((runtime, query, delay))
}
#[path = "persistent_goal/runner.rs"]
mod runner;
#[path = "persistent_goal/session.rs"]
mod session;

pub fn qualify(options: &Options) -> Evidence {
    let started = Instant::now();
    let mut evidence = Evidence {
        start_unix_ms: options.start_unix_ms,
        ..Default::default()
    };
    match runner::run(options, &mut evidence) {
        Ok(()) => evidence.passed = true,
        Err(error) => evidence.error = Some(error),
    }
    evidence.wall_elapsed_ms = started.elapsed().as_millis();
    evidence
}

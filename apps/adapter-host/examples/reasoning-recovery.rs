//! Fault-injection demo using the existing solver and model fixture processes.
use arcs::adapters::CapabilityRef;
use arcs::reasoning::attempts::AttemptQuery;
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use serde_json::json;
use std::path::Path;
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: reasoning-recovery <host.json> <runtime.json> <solver.sqlite>".into());
    }
    let mut host = Host::open(Path::new(&args[0]))?;
    let mut runtime = TaskRuntime::open(&host, RuntimeConfig::load(Path::new(&args[1]))?)?;
    // Hold only the demo solver's own journal after its metadata handshake.
    // Its normal V2 request will exceed its budget; the Core must record that
    // failure before the configured alternative can receive a new request.
    let lock = rusqlite::Connection::open(Path::new(&args[2]))?;
    lock.execute_batch("BEGIN EXCLUSIVE")?;
    let report = runtime.run(&mut host, Some(30), || false)?;
    lock.execute_batch("ROLLBACK")?;
    let recoveries = runtime.reasoning_recoveries(&host)?;
    let history = host.reasoning_attempts(&AttemptQuery {
        capabilities: vec![
            CapabilityRef::new("example.solver", "solution.propose"),
            CapabilityRef::new("example.model-fixture", "solution.propose"),
        ],
        after_sequence: 0,
        limit: 16,
    })?;
    host.shutdown()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"runtime":report,"reasoning_recoveries":recoveries,"history":history})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("reasoning recovery: {error}");
        std::process::exit(1);
    }
}

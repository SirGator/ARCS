//! Release an injected solver lock after its first timeout, then let the normal
//! background connector and explicitly bounded request retry repair the task.
use arcs::adapters::CapabilityRef;
use arcs::reasoning::attempts::AttemptQuery;
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use serde_json::json;
use std::{path::Path, time::Duration};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: reasoner-reconnect <host.json> <runtime.json> <solver.sqlite>".into());
    }
    let mut host = Host::open(Path::new(&args[0]))?;
    let mut runtime = TaskRuntime::open(&host, RuntimeConfig::load(Path::new(&args[1]))?)?;
    let lock = rusqlite::Connection::open(Path::new(&args[2]))?;
    lock.execute_batch("BEGIN EXCLUSIVE")?;
    let query = AttemptQuery {
        capabilities: vec![
            CapabilityRef::new("example.solver", "solution.propose"),
            CapabilityRef::new("example.model-fixture", "solution.propose"),
        ],
        after_sequence: 0,
        limit: 16,
    };
    let mut released = false;
    let mut ticks = 0;
    while ticks < 150 {
        runtime.tick(&mut host)?;
        ticks += 1;
        if !released && host.reasoning_attempts(&query)?.page_counts.failures > 0 {
            lock.execute_batch("ROLLBACK")?;
            released = true;
        }
        if released && runtime.report(ticks)?.ready_for_review == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !released {
        lock.execute_batch("ROLLBACK")?;
    }
    let report = runtime.report(ticks)?;
    let history = host.reasoning_attempts(&query)?;
    let recoveries = runtime.reasoning_recoveries(&host)?;
    let reconnects = host.adapter_reconnections()?;
    host.shutdown()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"runtime":report,"history":history,"reasoning_recoveries":recoveries,"adapter_reconnections":reconnects})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("reasoner reconnect: {error}");
        std::process::exit(1);
    }
}

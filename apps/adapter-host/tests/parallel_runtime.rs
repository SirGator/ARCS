#[path = "support/memory.rs"]
mod memory;
#[path = "support/parallel.rs"]
mod parallel_support;
mod support;
use parallel_support::Fixture;

use std::time::{Duration, Instant};

use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::json;

fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    mut condition: impl FnMut(&TaskRuntime) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        runtime.tick(host).unwrap();
        if condition(runtime) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "parallel runtime made no progress: {:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn slow_multirole_reasoner_does_not_block_two_other_tasks_or_observations() {
    let f = Fixture::new();
    for (id, mode, observes) in [
        ("slow", "gate", true),
        ("fast", "normal", false),
        ("third", "normal", true),
    ] {
        f.peer(id, mode, observes);
        f.route(id, id);
    }
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2 && f.started("slow")
    });
    let revision = host.report().unwrap().world_revision;
    assert!(revision > 0);
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert!(host.report().unwrap().world_revision > revision);
    assert_eq!(f.count("slow", "attempts"), 1);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 1);
    f.release("slow");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    for event in host.store().committed_after(0).unwrap() {
        assert!(
            !["approval", "action", "execution"].contains(&event.artifact.artifact_type.as_str())
        );
        if event.artifact.artifact_type.ends_with("_candidate") {
            assert_eq!(event.artifact.trust.level, arcs::core::TrustLevel::Low);
        }
        if event.artifact.schema_id.0 == "arcs.reasoning_request.v2" {
            assert!(
                event.artifact.payload["invocation"]["context"][0]["payload"]
                    .get("private_note")
                    .is_none()
            );
        }
    }
    host.shutdown().unwrap();
}

#[test]
fn worker_capacity_and_single_session_reservations_are_enforced() {
    let f = Fixture::new();
    for id in ["a", "b", "c"] {
        f.peer(id, "gate", false);
        f.route(id, id);
    }
    f.route("same-a", "a");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    for _ in 0..5 {
        runtime.tick(&mut host).unwrap();
    }
    assert!(!f.started("c"));
    assert_eq!(f.count("a", "attempts"), 1);
    f.release("b");
    until(&mut host, &mut runtime, |_| f.started("c"));
    assert_eq!(f.count("a", "attempts"), 1);
    f.release("a");
    f.release("c");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert_eq!(f.count("a", "results"), 2);
    host.shutdown().unwrap();
}

#[test]
fn component_failure_is_isolated_and_not_automatically_retried() {
    let f = Fixture::new();
    f.peer("broken", "fail", true);
    f.route("broken", "broken");
    f.peer("working", "normal", false);
    f.route("working", "working");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        let s = r.report(0).unwrap();
        s.failed_jobs == 1 && s.ready_for_review == 1
    });
    for _ in 0..5 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(f.count("broken", "attempts"), 1);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    f.write("event.json", json!({"id":"second","subject":"second","reference":"second","payload":{"values":[3,7],"target":6}}));
    let queued = std::process::Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(f.directory.join("input-profile.json"))
        .arg(f.directory.join("input.sqlite"))
        .arg(f.directory.join("event.json"))
        .output()
        .unwrap();
    assert!(
        queued.status.success(),
        "{}",
        String::from_utf8_lossy(&queued.stderr)
    );
    until(&mut host, &mut runtime, |r| {
        let s = r.report(0).unwrap();
        s.failed_jobs == 2 && s.ready_for_review == 2
    });
    assert_eq!(f.count("broken", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn invalid_worker_payload_is_rejected_without_stopping_other_tasks() {
    let f = Fixture::new();
    f.peer("invalid", "invalid", false);
    f.route("invalid", "invalid");
    f.peer("working", "normal", false);
    f.route("working", "working");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        let s = r.report(0).unwrap();
        s.failed_jobs == 1 && s.ready_for_review == 1
    });
    let done = runtime.completed_after(0, 10).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].route_id, "working");
    assert_eq!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .filter(|e| e.artifact.artifact_type.ends_with("_candidate"))
            .count(),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn timeout_releases_capacity_and_keeps_independent_work_running() {
    let f = Fixture::new();
    f.peer("slow", "gate", true);
    f.route("slow", "slow");
    f.peer("working", "normal", false);
    f.route("working", "working");
    let mut config = f.read("runtime.json");
    config["routes"][0]["budget"]["max_runtime_ms"] = json!(250);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        let s = r.report(0).unwrap();
        s.failed_jobs == 1 && s.ready_for_review == 1
    });
    for _ in 0..5 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    let started = Instant::now();
    host.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn shutdown_interrupts_a_worker_and_restart_reuses_its_journaled_result() {
    let f = Fixture::new();
    f.peer("slow", "gate", false);
    f.route("slow", "slow");
    f.peer("working", "normal", false);
    f.route("working", "working");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("slow") && r.report(0).unwrap().ready_for_review == 1
    });
    let first = runtime.completed_after(0, 10).unwrap();
    let started = Instant::now();
    host.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(runtime);
    drop(host);
    assert_eq!(f.count("slow", "results"), 1);
    f.release("slow");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert!(runtime.completed_after(0, 10).unwrap().contains(&first[0]));
    assert_eq!(f.count("slow", "results"), 1);
    assert_eq!(f.count("slow", "attempts"), 2);
    assert_eq!(f.count("working", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn invalid_concurrency_is_rejected_before_input_is_acknowledged() {
    let f = Fixture::new();
    f.peer("model", "normal", false);
    f.route("model", "model");
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    for limit in [0, 33] {
        let mut config = RuntimeConfig::load(f.directory.join("runtime.json")).unwrap();
        config.max_concurrent_reasoning = limit;
        assert!(TaskRuntime::open(&host, config).is_err());
    }
    let count: i64 = rusqlite::Connection::open(f.directory.join("input.sqlite"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events WHERE ack=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    host.shutdown().unwrap();
}

#[test]
fn memory_selection_stays_fixed_while_parallel_work_is_pending() {
    let f = Fixture::new();
    f.peer("slow", "gate", false);
    f.route("slow", "slow");
    let mut config = f.read("runtime.json");
    config["routes"][0]["memory"] = memory::policy("project:parallel");
    config["routes"][0]["budget"]["max_context_items"] = json!(3);
    f.write("runtime.json", config);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let original = memory::append(&host, "aaa-original-memory", "project:parallel");
    let mut runtime = TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    )
    .unwrap();
    until(&mut host, &mut runtime, |_| f.started("slow"));
    let later = memory::append(&host, "later-memory", "project:parallel");
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    f.release("slow");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    let done = runtime.completed_after(0, 10).unwrap().remove(0);
    let audit = host.store().get(&done.request_version_id).unwrap().unwrap();
    let context = audit.payload["invocation"]["context"].as_array().unwrap();
    assert_eq!(context.len(), 2);
    assert!(
        context
            .iter()
            .any(|item| item["version_id"] == original.version_id.0)
    );
    assert!(
        context
            .iter()
            .all(|item| item["version_id"] != later.version_id.0)
    );
    assert_eq!(memory::snapshots(&host).len(), 1);
    assert_eq!(f.count("slow", "attempts"), 1);
    host.shutdown().unwrap();
}

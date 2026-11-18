#[path = "support/parallel.rs"]
mod parallel_support;
mod support;

use arcs::adapters::{AdapterId, CapabilityId, CapabilityRef};
use arcs::core::{Actor, ActorType};
use arcs::reasoning::attempts::{AttemptOutcome, AttemptQuery, ReasoningFailureKind};
use arcs_host::{
    Host,
    task_runtime::{
        TaskRuntime,
        suspension::{TaskChange, TaskControlCommand},
    },
};
use parallel_support::Fixture;
use serde_json::json;
use std::time::{Duration, Instant};

fn query(ids: &[&str], limit: usize) -> AttemptQuery {
    AttemptQuery {
        capabilities: ids
            .iter()
            .map(|id| CapabilityRef {
                adapter_id: AdapterId((*id).into()),
                capability_id: CapabilityId("solution.propose".into()),
            })
            .collect(),
        after_sequence: 0,
        limit,
    }
}
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    condition: impl Fn(&Host, &TaskRuntime) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        runtime.tick(host).unwrap();
        if condition(host, runtime) {
            return;
        }
        assert!(Instant::now() < end, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn real_v2_processes_record_valid_no_solution_and_failed_outcomes_without_payload_export() {
    let f = Fixture::new();
    for (id, mode) in [
        ("good", "normal"),
        ("bad", "fail"),
        ("invalid", "invalid"),
        ("none", "no_solution"),
    ] {
        f.peer(id, mode, false);
        f.route(id, id);
    }
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(4);
    for route in config["routes"].as_array_mut().unwrap() {
        route["payload_fields"] = json!(["values", "target", "private_note"]);
    }
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["private_note"] = json!("HISTORY_PRIVATE_INPUT");
    f.write("input-profile.json", input);
    let script = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&script).unwrap();
    std::fs::write(
        script,
        source.replace("injected component failure", "HISTORY_SECRET_ERROR"),
    )
    .unwrap();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, runtime| {
        let report = runtime.report(0).unwrap();
        report.ready_for_review == 2 && report.failed_jobs == 2
    });
    let page = host
        .reasoning_attempts(&query(&["good", "bad", "invalid", "none"], 10))
        .unwrap();
    assert_eq!(page.attempts.len(), 4);
    assert_eq!(page.page_counts.valid_responses, 2);
    assert_eq!(page.page_counts.solved_candidate_responses, 1);
    assert_eq!(page.page_counts.failures, 2);
    assert_eq!(page.page_counts.retryable_failures, 1);
    assert!(
        page.attempts
            .iter()
            .all(|entry| entry.attempt.host_completion_latency_ms.is_some())
    );
    assert!(page.attempts.iter().any(|entry| matches!(
        entry.attempt.outcome,
        AttemptOutcome::Failed {
            failure: ReasoningFailureKind::InvalidResponse,
            ..
        }
    )));
    let encoded = serde_json::to_string(&page).unwrap();
    assert!(
        !encoded.contains("HISTORY_PRIVATE_INPUT") && !encoded.contains("HISTORY_SECRET_ERROR")
    );
    assert!(!page.candidate_quality_verified);
    let versions: Vec<_> = page
        .attempts
        .iter()
        .map(|entry| entry.version_id.clone())
        .collect();
    let before = host.store().len().unwrap();
    host.reasoning_attempts(&query(&["good"], 10)).unwrap();
    assert_eq!(host.store().len().unwrap(), before);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let page = host
        .reasoning_attempts(&query(&["good", "bad", "invalid", "none"], 10))
        .unwrap();
    assert_eq!(
        versions,
        page.attempts
            .iter()
            .map(|entry| entry.version_id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(f.count("good", "attempts"), 1);
    assert_eq!(f.count("bad", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn pages_merge_capability_scopes_and_current_grants_are_checked_before_reading() {
    let f = Fixture::new();
    f.peer("a", "normal", true);
    f.peer("b", "normal", false);
    f.route("a", "a");
    f.route("b", "b");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    let mut q = query(&["b", "a"], 1);
    let first = host.reasoning_attempts(&q).unwrap();
    assert!(first.has_more);
    assert_eq!(first.attempts.len(), 1);
    q.after_sequence = first.next_sequence;
    let second = host.reasoning_attempts(&q).unwrap();
    assert!(!second.has_more);
    assert_eq!(second.attempts.len(), 1);
    assert!(second.attempts[0].sequence > first.attempts[0].sequence);
    assert_eq!(second.next_sequence, second.through_sequence);
    assert_ne!(
        first.attempts[0].attempt.capability,
        second.attempts[0].attempt.capability
    );
    let scoped = host.reasoning_attempts(&query(&["a"], 10)).unwrap();
    assert_eq!(scoped.attempts.len(), 1);
    assert_eq!(scoped.attempts[0].attempt.capability.adapter_id.0, "a");
    let mut not_reason = query(&["a"], 10);
    not_reason.capabilities[0].capability_id.0 = "status".into();
    assert!(host.reasoning_attempts(&not_reason).is_err());
    assert!(
        host.reasoning_attempts(&query(&["unregistered"], 10))
            .is_err()
    );
    host.shutdown().unwrap();
}

#[test]
fn paused_completion_is_evidence_without_claiming_a_task_handoff_or_goal_success() {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.route("main", "a");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| f.started("a"));
    assert!(
        host.reasoning_attempts(&query(&["a"], 10))
            .unwrap()
            .attempts
            .is_empty()
    );
    runtime
        .control_task(
            &mut host,
            TaskControlCommand {
                sequence: 1,
                expected_revision: 0,
                command_id: "pause".into(),
                operator: Actor {
                    actor_type: ActorType::Human,
                    id: "operator".into(),
                },
                reason: "pause the task".into(),
                change: TaskChange::Pause,
            },
        )
        .unwrap();
    f.release("a");
    until(&mut host, &mut runtime, |host, _| {
        !host
            .reasoning_attempts(&query(&["a"], 10))
            .unwrap()
            .attempts
            .is_empty()
    });
    let report = runtime.report(0).unwrap();
    assert_eq!(report.paused_jobs, 1);
    assert_eq!(report.ready_for_review, 0);
    let page = host.reasoning_attempts(&query(&["a"], 10)).unwrap();
    assert_eq!(page.page_counts.solved_candidate_responses, 1);
    assert!(!page.candidate_quality_verified);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_lost_task_completion_checkpoint_does_not_lose_or_duplicate_core_attempt_evidence() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("main", "a");
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER lost_completion BEFORE UPDATE ON runtime_jobs WHEN NEW.state='ready_for_review' BEGIN SELECT RAISE(ABORT,'lost task completion'); END;").unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    let page = host.reasoning_attempts(&query(&["a"], 10)).unwrap();
    assert_eq!(page.attempts.len(), 1);
    let id = page.attempts[0].version_id.clone();
    db.execute_batch("DROP TRIGGER lost_completion").unwrap();
    drop(db);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let page = host.reasoning_attempts(&query(&["a"], 10)).unwrap();
    assert_eq!(page.attempts[0].version_id, id);
    assert_eq!(page.attempts.len(), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn readonly_cli_never_starts_runtime_work_and_invalid_queries_fail_before_host_start() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("main", "a");
    let run = |file: &str| {
        std::process::Command::new(support::adapter_binary("arcs-host"))
            .arg("--config")
            .arg(f.directory.join("host.json"))
            .arg("--reasoning-attempts")
            .arg(f.directory.join(file))
            .output()
            .unwrap()
    };
    f.write("bad.json", json!({"capabilities":[],"limit":1}));
    assert!(!run("bad.json").status.success());
    assert!(!f.directory.join("core.sqlite").exists());
    f.write(
        "query.json",
        serde_json::to_value(query(&["a"], 10)).unwrap(),
    );
    let result = run("query.json");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let page: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(page["scanned"], 0);
    assert_eq!(f.count("a", "attempts"), 0);
    assert!(!f.directory.join("runtime.sqlite").exists());
    let mixed = std::process::Command::new(support::adapter_binary("arcs-host"))
        .arg("--config")
        .arg(f.directory.join("host.json"))
        .arg("--reasoning-attempts")
        .arg(f.directory.join("query.json"))
        .arg("--runtime")
        .arg(f.directory.join("runtime.json"))
        .output()
        .unwrap();
    assert!(!mixed.status.success());
    assert!(!f.directory.join("runtime.sqlite").exists());
}

#[test]
fn external_adapters_cannot_use_the_core_attempt_publisher() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    let mut grant = f.read("a-grant.json");
    grant["adapter_id"] = json!(arcs::reasoning::attempts::PUBLISHER);
    f.write("a-grant.json", grant);
    assert!(Host::open(f.directory.join("host.json")).is_err());
    assert!(!f.directory.join("core.sqlite").exists());
}

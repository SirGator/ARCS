#[path = "../support/parallel.rs"]
mod parallel;

use super::{Fixture, MONTH, NOW, assert_no_authority};
use crate::support;
use arcs::core::{Actor, ActorType};
use arcs_host::{
    Host,
    task_runtime::{
        TaskRuntime,
        scheduler::{SeriesChange, SeriesControlCommand, SeriesState},
        suspension::{TaskChange, TaskControlCommand},
    },
};
use serde_json::{Value, json};
use std::process::Command;
use std::time::{Duration, Instant};

fn command(sequence: u64, revision: u64, id: &str, change: SeriesChange) -> SeriesControlCommand {
    SeriesControlCommand {
        sequence,
        expected_revision: revision,
        command_id: id.into(),
        operator: Actor {
            actor_type: ActorType::Human,
            id: "operator.test".into(),
        },
        reason: "operator controls check series".into(),
        change,
    }
}

fn cli(f: &Fixture, command: &SeriesControlCommand) -> Value {
    f.write("command.json", json!(command));
    let result = Command::new(support::adapter_binary("arcs-host"))
        .arg("--config")
        .arg(f.directory.join("host.json"))
        .arg("--runtime")
        .arg(f.directory.join("runtime.json"))
        .arg("--series-command")
        .arg(f.directory.join("command.json"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn a_dormant_series_can_be_paused_via_cli_and_resumes_on_its_original_grid() {
    let f = Fixture::new(Value::Null);
    f.recurring(5);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    let first = runtime
        .tick_at(&mut host, NOW)
        .unwrap()
        .ready_for_review
        .remove(0);
    host.shutdown().unwrap();
    drop((runtime, host));
    // Reopen an operational journal from before series controls were introduced.
    f.db("runtime")
        .execute_batch(
            "DROP TABLE runtime_check_control_events;
             DROP INDEX runtime_check_active_due;
             ALTER TABLE runtime_check_series DROP COLUMN control_reason;
             ALTER TABLE runtime_check_series DROP COLUMN control_state;
             ALTER TABLE runtime_check_series DROP COLUMN control_revision;
             CREATE INDEX runtime_check_due ON runtime_check_series(next_due,sequence);",
        )
        .unwrap();
    let pause = command(first.sequence, 0, "pause", SeriesChange::Pause);
    let paused = cli(&f, &pause);
    assert_eq!(paused["series_control"]["state"], "paused");
    assert_eq!(paused["recurring_checks"][0]["control_revision"], 1);
    assert!(paused["next_wake_unix_ms"].is_null());
    assert_eq!(paused["capacity_used"], 0);
    assert_eq!(cli(&f, &pause)["series_control"], paused["series_control"]);

    let (mut host, mut runtime) = f.open();
    // A paused, overdue series does not take the single free slot from new work.
    f.enqueue("input", "unrelated", "project:independent");
    host.poll_inputs().unwrap();
    let late = NOW + 3 * MONTH + 1;
    let tick = runtime.tick_at(&mut host, late).unwrap();
    assert!(tick.recurring_jobs.is_empty());
    assert_eq!(tick.ready_for_review.len(), 1);
    assert_ne!(
        tick.ready_for_review[0].input_version_id,
        first.input_version_id
    );
    assert!(
        runtime
            .control_check_series(
                &mut host,
                command(first.sequence, 0, "stale", SeriesChange::Resume)
            )
            .is_err()
    );
    let mut reused = pause.clone();
    reused.reason = "different command".into();
    assert!(runtime.control_check_series(&mut host, reused).is_err());
    assert!(
        runtime
            .control_check_series(&mut host, command(900, 0, "missing", SeriesChange::Pause))
            .is_err()
    );
    let mut unauthorized = command(first.sequence, 1, "model", SeriesChange::Resume);
    unauthorized.operator.actor_type = ActorType::Model;
    assert!(
        runtime
            .control_check_series(&mut host, unauthorized)
            .is_err()
    );
    assert_eq!(
        runtime
            .series_control_history(first.sequence, 0, 10)
            .unwrap()
            .len(),
        1
    );
    host.shutdown().unwrap();
    drop((runtime, host));

    let resume = command(first.sequence, 1, "resume", SeriesChange::Resume);
    let resumed = cli(&f, &resume);
    assert_eq!(resumed["series_control"]["state"], "active");
    assert_eq!(resumed["next_wake_unix_ms"], NOW + MONTH);
    assert_eq!(cli(&f, &pause)["series_control"], paused["series_control"]);
    let (mut host, mut runtime) = f.open();
    let tick = runtime.tick_at(&mut host, late).unwrap();
    assert_eq!(tick.recurring_jobs.len(), 1);
    assert_eq!(
        tick.ready_for_review[0].input_version_id,
        first.input_version_id
    );
    let occurrence = runtime
        .check_occurrences_after(first.sequence, first.sequence, 1)
        .unwrap()
        .remove(0);
    assert_eq!(
        (
            occurrence.occurrence,
            occurrence.due_unix_ms,
            occurrence.skipped_before
        ),
        (3, NOW + 3 * MONTH, 2)
    );
    assert_eq!(
        runtime.completed_after(0, 1).unwrap(),
        std::slice::from_ref(&first)
    );
    let resume_event = runtime
        .series_control_history(first.sequence, 1, 1)
        .unwrap()
        .remove(0);
    assert_eq!(resume_event.command, resume);
    assert_eq!(resume_event.receipt.revision, 2);
    assert_eq!(f.invocations(), 3);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

fn gated_fixture(with_independent: bool) -> parallel::Fixture {
    let f = parallel::Fixture::new();
    f.peer("a", "gate", false);
    f.route("checks", "a");
    if with_independent {
        f.route("independent", "a");
    }
    let mut config = f.read("runtime.json");
    config["routes"][0]["recurrence"] = json!({"every_ms": MONTH, "max_occurrences": 5});
    if with_independent {
        config["routes"][1]["activation"] = json!({"kind":"at_time","unix_ms":NOW+MONTH});
    }
    f.write("runtime.json", config);
    let mut host = f.read("host.json");
    host["resource_pools"] = json!({"compute":1});
    host["adapters"][1]["reasoning_resources"] = json!({"compute":1});
    f.write("host.json", host);
    f
}

fn tick_until(
    host: &mut Host,
    run: &mut TaskRuntime,
    now: u64,
    done: impl Fn(&Host, &TaskRuntime) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(host, run) {
        let tick = run.tick_at(host, now).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        assert!(Instant::now() < deadline, "{:?}", run.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn a_paused_series_captures_the_live_result_and_respects_an_independent_task_pause() {
    let f = gated_fixture(false);
    let (mut host, mut runtime) = f.open();
    tick_until(&mut host, &mut runtime, NOW, |_, _| f.started("a"));
    let pause = command(1, 0, "pause-series", SeriesChange::Pause);
    runtime
        .control_check_series(&mut host, pause.clone())
        .unwrap();
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 1);
    f.release("a");
    tick_until(&mut host, &mut runtime, NOW + MONTH, |host, _| {
        host.resource_usage()[0].used == 0
    });
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 0);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(runtime.check_occurrences_after(1, 0, 10).unwrap().len(), 1);
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    let mut independent_pause = TaskControlCommand {
        sequence: 1,
        expected_revision: 0,
        command_id: "pause-task".into(),
        operator: pause.operator,
        reason: "hold individual occurrence".into(),
        change: TaskChange::Pause,
    };
    runtime
        .control_task(&mut host, independent_pause.clone())
        .unwrap();
    runtime
        .control_check_series(
            &mut host,
            command(1, 1, "resume-series", SeriesChange::Resume),
        )
        .unwrap();
    let tick = runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    assert!(tick.ready_for_review.is_empty() && tick.recurring_jobs.is_empty());
    independent_pause.expected_revision = 1;
    independent_pause.command_id = "resume-task".into();
    independent_pause.change = TaskChange::Resume {
        recover_dispatched: false,
    };
    runtime.control_task(&mut host, independent_pause).unwrap();
    let ready = runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    assert_eq!(ready.ready_for_review[0].sequence, 1);
    assert!(ready.recurring_jobs.is_empty());
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn series_cancellation_commits_before_reaping_a_late_reply_and_stays_terminal() {
    let f = gated_fixture(true);
    let (mut host, mut runtime) = f.open();
    tick_until(&mut host, &mut runtime, NOW, |_, _| f.started("a"));
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER interrupt_series_cancel BEFORE INSERT ON runtime_check_control_events
        BEGIN SELECT RAISE(ABORT,'test series control checkpoint failure'); END;",
    )
    .unwrap();
    let cancel = command(1, 0, "cancel-series", SeriesChange::Cancel);
    assert!(
        runtime
            .control_check_series(&mut host, cancel.clone())
            .is_err()
    );
    let after_rollback = runtime.report(0).unwrap();
    assert_eq!(
        after_rollback.recurring_checks[0].state,
        SeriesState::Active
    );
    assert_eq!(after_rollback.recurring_checks[0].last_state, "running");
    assert_eq!(after_rollback.next_wake_unix_ms, Some(NOW + MONTH));
    assert!(runtime.series_control_history(1, 0, 10).unwrap().is_empty());
    db.execute_batch("DROP TRIGGER interrupt_series_cancel")
        .unwrap();
    let receipt = runtime
        .control_check_series(&mut host, cancel.clone())
        .unwrap();
    assert_eq!(receipt.cancelled_occurrence, Some(1));
    let stopped = runtime.report(0).unwrap();
    assert_eq!(stopped.recurring_checks[0].state, SeriesState::Cancelled);
    assert_eq!(stopped.recurring_checks[0].last_state, "cancelled");
    assert!(stopped.recurring_checks[0].next_due_unix_ms.is_none());
    assert!(
        runtime
            .control_check_series(&mut host, command(1, 1, "revive", SeriesChange::Resume))
            .is_err()
    );
    f.release("a");
    tick_until(&mut host, &mut runtime, NOW + MONTH, |_, run| {
        run.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(
        runtime.completed_after(0, 10).unwrap()[0].route_id,
        "independent"
    );
    assert_eq!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .filter(|event| event.artifact.schema_id.0 == "arcs.reasoning_result.v2")
            .count(),
        1
    );
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    assert_eq!(
        runtime.control_check_series(&mut host, cancel).unwrap(),
        receipt
    );
    assert!(
        runtime
            .tick_at(&mut host, NOW + 6 * MONTH)
            .unwrap()
            .recurring_jobs
            .is_empty()
    );
    assert_eq!(runtime.check_occurrences_after(1, 0, 10).unwrap().len(), 1);
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
}

#[test]
fn cancelling_a_live_occurrence_is_atomic_and_keeps_the_series_on_its_grid() {
    let f = gated_fixture(true);
    let (mut host, mut runtime) = f.open();
    tick_until(&mut host, &mut runtime, NOW, |_, _| f.started("a"));
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER interrupt_task_cancel BEFORE INSERT ON runtime_task_control_events
         BEGIN SELECT RAISE(ABORT,'test task control checkpoint failure'); END;",
    )
    .unwrap();
    let cancel = TaskControlCommand {
        sequence: 1,
        expected_revision: 0,
        command_id: "cancel-first-occurrence".into(),
        operator: Actor {
            actor_type: ActorType::Human,
            id: "operator.test".into(),
        },
        reason: "stop only this occurrence".into(),
        change: TaskChange::Cancel,
    };
    assert!(runtime.control_task(&mut host, cancel.clone()).is_err());
    assert!(runtime.task_control_history(1, 0, 10).unwrap().is_empty());
    assert_eq!(
        runtime.report(0).unwrap().recurring_checks[0].last_state,
        "running"
    );
    db.execute_batch("DROP TRIGGER interrupt_task_cancel")
        .unwrap();
    let receipt = runtime.control_task(&mut host, cancel.clone()).unwrap();
    assert!(receipt.cancelled);
    let stopped = runtime.report(0).unwrap();
    assert_eq!(stopped.recurring_checks[0].state, SeriesState::Active);
    assert_eq!(stopped.recurring_checks[0].last_state, "cancelled");
    assert_eq!(
        stopped.recurring_checks[0].next_due_unix_ms,
        Some(NOW + MONTH)
    );

    f.release("a");
    tick_until(&mut host, &mut runtime, NOW + MONTH, |_, run| {
        run.report(0).unwrap().ready_for_review == 2
    });
    let report = runtime.report(0).unwrap();
    assert_eq!(
        report.recurring_checks[0].next_due_unix_ms,
        Some(NOW + 2 * MONTH)
    );
    assert_eq!(f.count("a", "attempts"), 3);
    assert_eq!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .filter(|event| event.artifact.schema_id.0 == "arcs.reasoning_result.v2")
            .count(),
        2
    );
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    let mut reused = cancel.clone();
    reused.reason = "changed reason".into();
    assert!(runtime.control_task(&mut host, reused).is_err());
    assert_eq!(
        json!(runtime.control_task(&mut host, cancel).unwrap()),
        json!(receipt)
    );
    assert_eq!(runtime.task_control_history(1, 0, 10).unwrap().len(), 1);
    assert_eq!(runtime.check_occurrences_after(1, 0, 10).unwrap().len(), 2);
    host.shutdown().unwrap();
}

#[test]
fn switching_to_synchronous_retry_cannot_revive_failed_occurrences_of_a_cancelled_series() {
    let f = gated_fixture(false);
    let mut peer = f.read("a.json");
    peer["mode"] = json!("invalid");
    f.write("a.json", peer);
    let (mut host, mut runtime) = f.open();
    for (time, expected) in [(NOW, 1), (NOW + MONTH, 2)] {
        let end = Instant::now() + Duration::from_secs(30);
        while runtime.report(0).unwrap().failed_jobs < expected {
            runtime.tick_at(&mut host, time).unwrap();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(3));
        }
    }
    assert_eq!(f.count("a", "attempts"), 2);
    runtime
        .control_check_series(
            &mut host,
            command(1, 0, "pause-failed", SeriesChange::Pause),
        )
        .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let set_parallelism = |value| {
        let mut config = f.read("runtime.json");
        config["max_concurrent_reasoning"] = json!(value);
        f.write("runtime.json", config);
    };
    set_parallelism(1);
    let (mut host, mut runtime) = f.open();
    // Only the latest failed occurrence is eligible for synchronous retry.
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 1);
    runtime.tick_at(&mut host, NOW + 3 * MONTH).unwrap();
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
    drop((runtime, host));

    set_parallelism(2);
    let (mut host, mut runtime) = f.open();
    let cancel = command(1, 1, "cancel-failed", SeriesChange::Cancel);
    let receipt = runtime.control_check_series(&mut host, cancel).unwrap();
    assert!(receipt.cancelled_occurrence.is_none());
    host.shutdown().unwrap();
    drop((runtime, host));

    set_parallelism(1);
    let (mut host, mut runtime) = f.open();
    let tick = runtime.tick_at(&mut host, NOW + 3 * MONTH).unwrap();
    assert!(tick.recurring_jobs.is_empty());
    let report = runtime.report(0).unwrap();
    assert_eq!(report.recurring_checks[0].state, SeriesState::Cancelled);
    assert_eq!(report.failed_jobs, 2);
    assert_eq!(report.pending_jobs, 0);
    assert_eq!(report.capacity_used, 0);
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
}

#[path = "support/goals.rs"]
mod goal_support;
#[path = "support/memory.rs"]
mod memory;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs_host::Host;
use arcs_host::task_runtime::goal::{GoalState, GoalTask};
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use goal_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const NOW: u64 = 1_800_000_000_000;
const MONTH: u64 = 30 * 24 * 60 * 60 * 1000;

fn configure(f: &Fixture, activation: Value, minimum: u64, parallel: bool) {
    let mut runtime = f.read("runtime.json");
    runtime["routes"][0]["goal"]["between_rounds"] = activation;
    runtime["max_pending_jobs"] = json!(1);
    runtime["max_concurrent_reasoning"] = json!(if parallel { 2 } else { 1 });
    f.write("runtime.json", runtime);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["minimum_actuations"] = json!(minimum);
    f.write("input-profile.json", input);
}
fn task(runtime: &TaskRuntime) -> GoalTask {
    runtime.goals_after(0, 10).unwrap().remove(0)
}
fn until_at(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    now: u64,
    predicate: impl Fn(GoalState) -> bool,
) -> GoalTask {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        runtime.tick_at(host, now).unwrap();
        if let Some(task) = runtime.goals_after(0, 10).unwrap().into_iter().next() {
            if predicate(task.state) {
                return task;
            }
            assert!(
                Instant::now() < deadline,
                "goal failed to progress: {task:?}"
            );
        } else {
            assert!(Instant::now() < deadline, "goal was not discovered");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn actor() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.test".into(),
    }
}
fn approve(host: &Host, runtime: &mut TaskRuntime, task: &GoalTask) {
    runtime
        .review_goal(
            host,
            task.sequence,
            task.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            actor(),
            "review this round".into(),
        )
        .unwrap();
}
fn actuations(f: &Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |r| r.get(0))
        .unwrap()
}
fn count(host: &Host, kind: &str) -> usize {
    host.store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.artifact.artifact_type == kind)
        .count()
}
fn invocations(f: &Fixture, name: &str) -> i64 {
    f.db(name)
        .query_row("SELECT COUNT(*) FROM invocations", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_goal_waits_between_rounds_then_reacquires_data_and_requires_a_new_manual_approval() {
    goal_reacquires_data_after_wait(
        json!({"kind":"after_delay","delay_ms":MONTH}),
        GoalState::WaitingTime,
    );
}

#[test]
fn without_a_signal_a_goal_reassesses_on_its_deadline_after_restart() {
    goal_reacquires_data_after_wait(
        json!({"kind":"on_event_or_after_delay","delay_ms":MONTH,"source":{"adapter_id":"goal.signal","capability_id":"task.submit"},"schema_id":"arcs.task.fan_goal.v1"}),
        GoalState::WaitingEvent,
    );
}

fn goal_reacquires_data_after_wait(activation: Value, wait_state: GoalState) {
    let f = Fixture::new();
    f.manual();
    f.with_observer(false);
    if wait_state == GoalState::WaitingEvent {
        add_signal(&f);
    }
    configure(&f, activation, 2, false);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = until_at(&mut host, &mut runtime, NOW, |s| {
        s == GoalState::AwaitingApproval
    });
    approve(&host, &mut runtime, &first);
    let waiting = until_at(&mut host, &mut runtime, NOW, |state| state == wait_state);
    assert_eq!(waiting.round, 2);
    assert!(waiting.outcome.is_some());
    assert_eq!(actuations(&f), 1);
    let report = runtime.report(0).unwrap();
    assert_eq!(
        report.waiting_time,
        u64::from(wait_state == GoalState::WaitingTime)
    );
    assert_eq!(
        report.waiting_event,
        u64::from(wait_state == GoalState::WaitingEvent)
    );
    assert_eq!(report.next_wake_unix_ms, Some(NOW + MONTH));
    let calls = invocations(&f, "planner") + invocations(&f, "assessor");
    for _ in 0..3 {
        runtime.tick_at(&mut host, NOW + MONTH - 1).unwrap();
    }
    assert_eq!(
        invocations(&f, "planner") + invocations(&f, "assessor"),
        calls
    );
    assert_eq!(task(&runtime).state, wait_state);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);

    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    let resumed = task(&runtime);
    assert_eq!(resumed.state, GoalState::Acquiring);
    let released = runtime
        .scheduled_history(resumed.sequence, None, 1)
        .unwrap()
        .remove(0);
    assert_eq!(released.released_at_unix_ms, Some(NOW + MONTH));
    assert_eq!(released.wake_event_version_id, None);
    assert!(resumed.before.is_empty());
    assert!(resumed.approval.is_none());
    let barrier = resumed.observation_cursor;
    // A crash after waking preserves the new freshness barrier.
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    assert_eq!(task(&runtime).observation_cursor, barrier);
    let second = until_at(&mut host, &mut runtime, NOW + MONTH, |s| {
        s == GoalState::AwaitingApproval
    });
    assert_eq!(second.before.len(), 2);
    let fresh = host.store().committed_after(barrier).unwrap();
    for version in &second.before {
        assert!(fresh.iter().any(|e| &e.artifact.version_id == version));
    }
    assert_ne!(first.candidate, second.candidate);
    assert_eq!(count(&host, "approval"), 1);
    assert_eq!(actuations(&f), 1);
    assert!(
        runtime
            .review_goal(
                &host,
                second.sequence,
                first.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                actor(),
                "late previous review".into()
            )
            .is_err()
    );
    approve(&host, &mut runtime, &second);
    let done = until_at(&mut host, &mut runtime, NOW + MONTH, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(actuations(&f), 2);
    assert_eq!(count(&host, "approval"), 2);
    assert_eq!(count(&host, "outcome"), 2);
    host.shutdown().unwrap();
}

#[test]
fn repeated_monthly_pauses_preserve_all_receipts_memory_and_effects_across_restarts() {
    let f = Fixture::new();
    configure(&f, json!({"kind":"after_delay","delay_ms":MONTH}), 4, true);
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"after_delay","delay_ms":0});
    config["routes"][0]["memory"] = memory::policy("project:long-goal");
    f.write("runtime.json", config);
    let mut host = f.host();
    let remembered = memory::append(&host, "experience-before-project", "project:long-goal");
    let mut runtime = f.runtime(&host);
    let mut now = NOW;
    for expected in 1..=4 {
        let saved = until_at(&mut host, &mut runtime, now, |s| {
            s == GoalState::WaitingTime || s.terminal()
        });
        assert_eq!(actuations(&f), expected);
        if expected == 4 {
            assert_eq!(saved.state, GoalState::Completed);
            break;
        }
        assert_eq!(saved.state, GoalState::WaitingTime);
        assert_eq!(saved.round, expected as u32 + 1);
        let schedule = runtime.scheduled_after(0, 1).unwrap().remove(0);
        assert_eq!(schedule.revision, expected as u32);
        assert_eq!(schedule.not_before_unix_ms, Some(now + MONTH));
        host.shutdown().unwrap();
        drop(runtime);
        drop(host);
        host = f.host();
        runtime = f.runtime(&host);
        runtime.tick_at(&mut host, now + MONTH - 1).unwrap();
        assert_eq!(actuations(&f), expected);
        now += MONTH;
    }
    let history = runtime.scheduled_history(1, None, 2).unwrap();
    assert_eq!(
        history.iter().map(|s| s.revision).collect::<Vec<_>>(),
        vec![0, 1]
    );
    let rest = runtime.scheduled_history(1, Some(1), 2).unwrap();
    assert_eq!(
        rest.iter().map(|s| s.revision).collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert!(rest.iter().all(|s| s.released_at_unix_ms.is_some()));
    assert_eq!(runtime.scheduled_after(0, 10).unwrap().len(), 1);
    assert_eq!(runtime.report(0).unwrap().waiting_time, 0);
    assert_eq!(count(&host, "action"), 4);
    assert_eq!(count(&host, "outcome"), 4);
    assert_eq!(invocations(&f, "planner"), 4);
    let snapshots = memory::snapshots(&host);
    assert_eq!(snapshots.len(), 8);
    for snapshot in snapshots {
        assert!(
            snapshot.payload["selected"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["selection"]["version_id"] == remembered.version_id.0)
        );
    }
    host.shutdown().unwrap();
}

fn add_signal(f: &Fixture) {
    let mut profile = f.read("input-profile.json");
    profile["adapter_id"] = json!("goal.signal");
    profile["events"] = json!([]);
    f.write("signal-profile.json", profile);
    let mut grant = f.read("input-grant.json");
    grant["adapter_id"] = json!("goal.signal");
    f.write("signal-grant.json", grant);
    let mut config = f.read("host.json");
    config["adapters"].as_array_mut().unwrap().push(json!({"program":support::adapter_binary("arcs-json-input"),"profile":"signal-profile.json","args":[{"path":"signal.sqlite"}],"grant_file":"signal-grant.json","timeout_ms":15000}));
    f.write("host.json", config);
}
fn signal(f: &Fixture, id: &str, subject: &str) {
    f.write("signal-event.json", json!({"id":id,"subject":subject,"reference":format!("signal:{id}"),"payload":{"device_id":"fan-01","enabled":false,"minimum_actuations":2}}));
    let result = std::process::Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(f.directory.join("signal-profile.json"))
        .arg(f.directory.join("signal.sqlite"))
        .arg(f.directory.join("signal-event.json"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn each_round_requires_a_new_matching_signal_at_capacity_and_can_be_cancelled_while_waiting() {
    let f = Fixture::new();
    add_signal(&f);
    configure(
        &f,
        json!({"kind":"on_event","source":{"adapter_id":"goal.signal","capability_id":"task.submit"},"schema_id":"arcs.task.fan_goal.v1","subject":"input:11:goal.signal:11:task.submit:11:mission:one"}),
        4,
        false,
    );
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    signal(&f, "too-early", "mission:one");
    let waiting = until_at(&mut host, &mut runtime, NOW, |s| {
        s == GoalState::WaitingEvent
    });
    assert_eq!(actuations(&f), 1);
    for _ in 0..3 {
        runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    }
    assert_eq!(task(&runtime).state, GoalState::WaitingEvent);
    signal(&f, "other-project", "mission:two");
    runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    assert_eq!(runtime.report(0).unwrap().waiting_event, 1);
    signal(&f, "resume-round-two", "mission:one");
    let deadline = Instant::now() + Duration::from_secs(20);
    let next = loop {
        runtime.tick_at(&mut host, NOW + MONTH).unwrap();
        let current = task(&runtime);
        if current.state == GoalState::WaitingEvent && current.round == 3 {
            break current;
        }
        assert!(
            !current.state.terminal() && Instant::now() < deadline,
            "{current:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(waiting.sequence, next.sequence);
    assert_eq!(actuations(&f), 2);
    for _ in 0..3 {
        runtime.tick_at(&mut host, NOW + 2 * MONTH).unwrap();
    }
    assert_eq!(task(&runtime).state, GoalState::WaitingEvent);
    let history = runtime.scheduled_history(next.sequence, None, 10).unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].wake_event_version_id.is_some());
    assert!(history[1].released_at_unix_ms.is_none());
    runtime
        .cancel_goal(&host, next.sequence, "operator stopped this project".into())
        .unwrap();
    signal(&f, "signal-after-cancel", "mission:one");
    runtime.tick_at(&mut host, NOW + 3 * MONTH).unwrap();
    assert_eq!(task(&runtime).state, GoalState::Cancelled);
    assert_eq!(actuations(&f), 2);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert_eq!(runtime.report(0).unwrap().waiting_event, 0);
    assert!(
        runtime.scheduled_history(next.sequence, None, 10).unwrap()[1]
            .released_at_unix_ms
            .is_none()
    );
    host.shutdown().unwrap();
}

#[test]
fn a_failed_wait_checkpoint_replays_the_assessment_without_repeating_the_action() {
    let f = Fixture::new();
    configure(&f, json!({"kind":"after_delay","delay_ms":MONTH}), 2, false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"after_delay","delay_ms":0});
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until_at(&mut host, &mut runtime, NOW, |s| s == GoalState::Assessing);
    f.db("runtime").execute_batch("CREATE TRIGGER reject_wait BEFORE INSERT ON runtime_job_schedules WHEN NEW.revision=1 BEGIN SELECT RAISE(ABORT,'injected checkpoint failure'); END;").unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick_at(&mut host, NOW));
    assert_eq!(task(&runtime).state, GoalState::Assessing);
    assert_eq!(runtime.scheduled_history(1, None, 10).unwrap().len(), 1);
    assert_eq!(actuations(&f), 1);
    assert_eq!(count(&host, "outcome"), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.db("runtime")
        .execute_batch("DROP TRIGGER reject_wait;")
        .unwrap();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until_at(&mut host, &mut runtime, NOW, |s| {
        s == GoalState::WaitingTime
    });
    assert_eq!(runtime.scheduled_history(1, None, 10).unwrap().len(), 2);
    let done = until_at(&mut host, &mut runtime, NOW + MONTH, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(actuations(&f), 2);
    assert_eq!(invocations(&f, "planner"), 2);
    assert_eq!(invocations(&f, "assessor"), 2);
    assert_eq!(count(&host, "outcome"), 2);
    host.shutdown().unwrap();
}

#[test]
fn zero_delay_repeats_remain_bounded_by_the_existing_round_budget() {
    let f = Fixture::new();
    configure(&f, json!({"kind":"after_delay","delay_ms":0}), 6, false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["max_rounds"] = json!(2);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let done = until_at(&mut host, &mut runtime, NOW, GoalState::terminal);
    assert_eq!(done.state, GoalState::BudgetExhausted);
    assert_eq!(done.round, 2);
    assert_eq!(actuations(&f), 2);
    assert_eq!(runtime.scheduled_history(1, None, 10).unwrap().len(), 1);
    assert_eq!(runtime.report(0).unwrap().waiting_time, 0);
    for _ in 0..3 {
        runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    }
    assert_eq!(actuations(&f), 2);
    host.shutdown().unwrap();
}

#[test]
fn an_initial_reassessment_can_finish_after_a_pause_without_an_obsolete_second_action() {
    let f = Fixture::new();
    configure(&f, json!({"kind":"after_delay","delay_ms":MONTH}), 2, false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["initial_assessment"] = json!(true);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until_at(&mut host, &mut runtime, NOW, |s| {
        s == GoalState::WaitingTime
    });
    assert_eq!(count(&host, "action"), 1);
    // An independent physical change while the project was asleep, injected
    // directly into the simulator's world rather than into ARCS evidence.
    f.db("fan")
        .execute("UPDATE fan_state SET enabled=0,actuation_count=2", [])
        .unwrap();
    let done = until_at(&mut host, &mut runtime, NOW + MONTH, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(count(&host, "action"), 1);
    assert_eq!(count(&host, "approval"), 1);
    assert_eq!(invocations(&f, "planner"), 1);
    let current = host.store().get(&done.before[0]).unwrap().unwrap();
    assert_eq!(current.payload["enabled"], false);
    assert_eq!(current.payload["actuation_count"], 2);
    host.shutdown().unwrap();
}

#[test]
fn invalid_round_activation_is_rejected_before_creating_the_runtime_journal() {
    let f = Fixture::new();
    configure(
        &f,
        json!({"kind":"on_event","source":{"adapter_id":"demo.fan","capability_id":"fan.set"},"schema_id":"arcs.state.fan.v1"}),
        2,
        false,
    );
    let mut host = f.host();
    assert!(
        TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
        )
        .is_err()
    );
    assert!(!f.directory.join("runtime.sqlite").exists());
    host.shutdown().unwrap();
}

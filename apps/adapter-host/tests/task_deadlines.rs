#[path = "support/goals.rs"]
mod goal_support;
#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs::runtime::reservations::ReservationState;
use arcs_host::{
    Host,
    task_runtime::{
        RuntimeConfig, TaskRuntime,
        goal::{GoalState, GoalTask},
        planning::PlanState,
    },
};
use parallel_support::Fixture;
use serde_json::json;
use std::time::{Duration, Instant};

fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    now: u64,
    condition: impl Fn(&TaskRuntime) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(25);
    loop {
        let tick = runtime.tick_at(host, now).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(runtime) {
            return;
        }
        assert!(Instant::now() < end, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn reopen(f: &Fixture, host: &Host) -> TaskRuntime {
    TaskRuntime::open(
        host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    )
    .unwrap()
}

fn reasoning() -> Fixture {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.route("a", "a");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(100);
    f.write("runtime.json", config);
    f
}

#[test]
fn a_month_long_budget_is_frozen_on_admission_and_expires_before_a_later_activation() {
    const MONTH: u64 = 30 * 24 * 60 * 60 * 1000;
    let f = reasoning();
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(MONTH);
    config["routes"][0]["activation"] = json!({"kind":"after_delay","delay_ms":2*MONTH});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, 1000).unwrap();
    let original = runtime.deadlines_after(0, 1).unwrap().remove(0);
    assert_eq!(original.accepted_at_unix_ms, 1000);
    assert_eq!(original.expires_at_unix_ms, 1000 + MONTH);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    assert_eq!(
        runtime.deadlines_after(0, 1).unwrap()[0].expires_at_unix_ms,
        original.expires_at_unix_ms
    );
    let tick = runtime.tick_at(&mut host, 1000 + MONTH).unwrap();
    assert_eq!(tick.expired_tasks, [1]);
    let report = runtime.report(0).unwrap();
    assert_eq!(
        (
            report.expired_jobs,
            report.cancelled_jobs,
            report.waiting_time
        ),
        (1, 1, 0)
    );
    assert_eq!(report.next_deadline_unix_ms, None);
    assert_eq!(f.count("a", "attempts"), 0);
    runtime.tick_at(&mut host, 0).unwrap();
    assert_eq!(
        runtime.deadlines_after(0, 1).unwrap()[0].expired_at_unix_ms,
        Some(1000 + MONTH)
    );
    host.shutdown().unwrap();
}

#[test]
fn expiry_discards_a_late_worker_result_but_keeps_its_capacity_until_the_reply() {
    let f = reasoning();
    f.peer("b", "normal", false);
    f.route("b", "b");
    let mut config = f.read("host.json");
    config["resource_pools"] = json!({"gpu":1});
    config["adapters"][1]["reasoning_resources"] = json!({"gpu":1});
    f.write("host.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, 1000, |r| {
        f.started("a") && r.report(0).unwrap().ready_for_review == 1
    });
    runtime.tick_at(&mut host, 1100).unwrap();
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 1);
    assert_eq!(host.resource_usage()[0].used, 1);
    f.release("a");
    let end = Instant::now() + Duration::from_secs(10);
    while host.resource_usage()[0].used != 0 {
        runtime.tick_at(&mut host, 1100).unwrap();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(host.resource_usage()[0].used, 0);
    let completed = runtime.completed_after(0, 10).unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].route_id, "b");
    let results = plan_support::artifacts(&host, "arcs.reasoning_result.v2");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].version_id, completed[0].result_version_id);
    host.shutdown().unwrap();
}

#[test]
fn a_plan_deadline_caps_children_and_cancels_open_work_while_preserving_finished_results() {
    let f = plan_support::fixture("gate", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(1000);
    config["routes"][0]["plan"]["templates"][0]["route"]["max_lifetime_ms"] = json!(5000);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, 1000, |r| {
        f.started("a") && r.report(0).unwrap().ready_for_review == 1
    });
    let completed = runtime.completed_after(0, 10).unwrap();
    let deadlines = runtime.deadlines_after(0, 10).unwrap();
    assert_eq!(deadlines.len(), 4);
    assert!(deadlines.iter().all(|d| d.expires_at_unix_ms == 2000));
    assert!(
        deadlines
            .iter()
            .skip(1)
            .all(|d| d.inherited_from == Some(1))
    );
    runtime.tick_at(&mut host, 2000).unwrap();
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(runtime.completed_after(0, 10).unwrap(), completed);
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 3);
    assert!(!f.started("join"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("a");
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, 1500).unwrap();
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 3);
    assert_eq!(runtime.completed_after(0, 10).unwrap(), completed);
    assert!(!f.started("join"));
    host.shutdown().unwrap();
}

#[test]
fn an_expiry_commit_failure_does_not_discard_a_worker_or_partially_cancel_a_plan() {
    let f = plan_support::fixture("gate", "gate");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(100);
    f.write("runtime.json", config);
    let mut host_config = f.read("host.json");
    host_config["resource_pools"] = json!({"compute":2});
    for index in [1, 2] {
        host_config["adapters"][index]["reasoning_resources"] = json!({"compute":1});
    }
    f.write("host.json", host_config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, 1000, |_| {
        f.started("a") && f.started("b")
    });
    let connection = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_expiry BEFORE UPDATE ON runtime_task_deadlines WHEN NEW.expired_at IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected expiry failure'); END;").unwrap();
    assert!(runtime.tick_at(&mut host, 1100).is_err());
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Running
    );
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 0);
    assert!(
        runtime
            .deadlines_after(0, 10)
            .unwrap()
            .iter()
            .all(|d| d.expired_at_unix_ms.is_none())
    );
    assert_eq!(host.resource_usage()[0].used, 2);
    connection
        .execute_batch("DROP TRIGGER reject_expiry;")
        .unwrap();
    // The accepted clock high-water mark survives the failed transition.
    runtime.tick_at(&mut host, 1000).unwrap();
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 4);
    host.shutdown().unwrap();
}

#[test]
fn a_shorter_template_budget_expires_only_its_step_and_blocks_the_dependent_plan() {
    let f = plan_support::fixture("gate", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(1000);
    config["routes"][0]["plan"]["templates"][0]["route"]["max_lifetime_ms"] = json!(100);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, 1000, |r| {
        f.started("a") && r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(
        runtime.deadlines_after(1, 1).unwrap()[0].expires_at_unix_ms,
        1100
    );
    until(&mut host, &mut runtime, 1100, |r| {
        r.plans_after(0, 1).unwrap()[0].state == PlanState::Blocked
    });
    let deadlines = runtime.deadlines_after(0, 10).unwrap();
    assert_eq!(
        deadlines
            .iter()
            .filter(|d| d.expired_at_unix_ms.is_some())
            .count(),
        1
    );
    assert_eq!(deadlines[0].expired_at_unix_ms, None);
    assert_eq!(runtime.completed_after(0, 10).unwrap().len(), 1);
    assert!(!f.started("join"));
    host.shutdown().unwrap();
}

#[test]
fn a_blocked_plan_with_an_outstanding_sibling_still_obeys_its_overall_deadline() {
    let f = plan_support::fixture("no_solution", "gate");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(100);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, 1000, |r| {
        f.started("b")
            && r.plans_after(0, 1)
                .unwrap()
                .first()
                .is_some_and(|p| p.state == PlanState::Blocked)
    });
    runtime.tick_at(&mut host, 1100).unwrap();
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 2);
    assert_eq!(runtime.completed_after(0, 10).unwrap().len(), 1);
    assert!(!f.started("join"));
    host.shutdown().unwrap();
}

fn goal_fixture() -> goal_support::Fixture {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(100);
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device.fan-01"]);
    f.write("runtime.json", config);
    f
}
fn goal(runtime: &TaskRuntime) -> GoalTask {
    runtime.goals_after(0, 1).unwrap().remove(0)
}
fn human() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.deadline-test".into(),
    }
}
fn approve(host: &Host, runtime: &mut TaskRuntime, task: &GoalTask) {
    runtime
        .review_goal(
            host,
            task.sequence,
            task.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "reviewed simulated action".into(),
        )
        .unwrap();
}
fn acts(f: &goal_support::Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn expiry_at_manual_review_releases_control_and_prevents_late_or_recovered_approval_from_acting() {
    for committed in [false, true] {
        let f = goal_fixture();
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        until(&mut host, &mut runtime, 1000, |r| {
            r.goals_after(0, 1)
                .unwrap()
                .iter()
                .any(|g| g.state == GoalState::AwaitingApproval)
        });
        let waiting = goal(&runtime);
        if committed {
            approve(&host, &mut runtime, &waiting);
            f.db("runtime")
                .execute(
                    "UPDATE runtime_jobs SET state='awaiting_approval',goal_json=?1",
                    [serde_json::to_string(&waiting).unwrap()],
                )
                .unwrap();
        }
        runtime.tick_at(&mut host, 1100).unwrap();
        assert_eq!(goal(&runtime).state, GoalState::Cancelled);
        assert_eq!(
            runtime.exclusive_resources(&host).unwrap()[0].state,
            ReservationState::Released
        );
        assert_eq!(acts(&f), 0);
        assert!(
            runtime
                .review_goal(
                    &host,
                    waiting.sequence,
                    waiting.candidate.as_ref().unwrap(),
                    ApprovalDecision::Approved,
                    human(),
                    "too late".into()
                )
                .is_err()
        );
        runtime.tick_at(&mut host, 1000).unwrap();
        assert_eq!(acts(&f), 0);
        assert_eq!(
            plan_support::artifacts(&host, "arcs.approval.v1").len(),
            usize::from(committed)
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn expiry_after_dispatch_keeps_an_unresolved_device_reservation_across_restart() {
    let f = goal_fixture();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, 1000, |r| {
        r.goals_after(0, 1)
            .unwrap()
            .iter()
            .any(|g| g.state == GoalState::AwaitingApproval)
    });
    let waiting = goal(&runtime);
    approve(&host, &mut runtime, &waiting);
    runtime.tick_at(&mut host, 1000).unwrap();
    assert_eq!(goal(&runtime).state, GoalState::Observing);
    assert_eq!(acts(&f), 1);
    runtime.tick_at(&mut host, 1100).unwrap();
    let reservation = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert_eq!(reservation.state, ReservationState::Held);
    assert_eq!(goal(&runtime).state, GoalState::Cancelled);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick_at(&mut host, 0).unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].version_id,
        reservation.version_id
    );
    assert_eq!(acts(&f), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_bounded_expiry_sweep_cannot_let_an_unswept_task_dispatch_at_its_deadline() {
    let f = reasoning();
    f.peer("b", "normal", false);
    f.route("b", "b");
    f.peer("c", "normal", false);
    f.route("c", "c");
    let mut config = f.read("runtime.json");
    config["max_jobs_per_tick"] = json!(1);
    for route in config["routes"].as_array_mut().unwrap() {
        route["max_lifetime_ms"] = json!(100);
        route["activation"] = json!({"kind":"at_time","unix_ms":1100});
    }
    config["routes"][2]["priority"] = json!(3);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, 1000).unwrap();
    let tick = runtime.tick_at(&mut host, 1100).unwrap();
    assert_eq!(tick.expired_tasks, [1, 3]);
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 2);
    for name in ["a", "b", "c"] {
        assert_eq!(f.count(name, "attempts"), 0);
    }
    runtime.tick_at(&mut host, 1100).unwrap();
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 3);
    host.shutdown().unwrap();
}

#[test]
fn default_identity_invalid_limits_and_deadline_overflow_are_checked_before_admission() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("a", "a");
    let (mut host, runtime) = f.open();
    drop(runtime);
    let count = host.report().unwrap().stored_artifacts;
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(null);
    f.write("runtime.json", config.clone());
    drop(reopen(&f, &host));
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    for limit in [0, 315_576_000_001u64] {
        config["routes"][0]["max_lifetime_ms"] = json!(limit);
        f.write("runtime.json", config.clone());
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err()
        );
    }
    host.shutdown().unwrap();
    let f = reasoning();
    let (mut host, mut runtime) = f.open();
    assert!(runtime.tick_at(&mut host, i64::MAX as u64 - 50).is_err());
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert!(runtime.deadlines_after(0, 10).unwrap().is_empty());
    assert!(runtime.deadlines_after(0, 0).is_err());
    assert!(!f.started("a"));
    host.shutdown().unwrap();
}

#[test]
fn live_time_is_rechecked_after_slow_polling_but_explicit_simulation_time_stays_fixed() {
    for simulated in [false, true] {
        let f = Fixture::new();
        f.peer("a", "normal", true);
        f.route("a", "a");
        let mut config = f.read("runtime.json");
        config["routes"][0]["max_lifetime_ms"] = json!(100);
        f.write("runtime.json", config);
        let path = f.directory.join("peer.py");
        let peer = std::fs::read_to_string(&path).unwrap().replace(
            "elif kind=='poll':",
            "elif kind=='poll':\n  time.sleep(.25)",
        );
        std::fs::write(path, peer).unwrap();
        let (mut host, mut runtime) = f.open();
        host.poll_inputs().unwrap();
        let tick = if simulated {
            runtime.tick_at(&mut host, 1000)
        } else {
            runtime.tick(&mut host)
        }
        .unwrap();
        if simulated {
            assert!(tick.expired_tasks.is_empty());
            assert_eq!(runtime.report(0).unwrap().expired_jobs, 0);
            assert_eq!(
                runtime.deadlines_after(0, 1).unwrap()[0].expires_at_unix_ms,
                1100
            );
        } else {
            assert_eq!(tick.expired_tasks, [1]);
            assert_eq!(runtime.report(0).unwrap().expired_jobs, 1);
            assert_eq!(f.count("a", "attempts"), 0);
        }
        host.shutdown().unwrap();
    }
}

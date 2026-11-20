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
        TaskRuntime,
        goal::{GoalState, GoalTask},
        suspension::*,
    },
};
use parallel_support::Fixture;
use serde_json::json;
use std::time::{Duration, Instant};

fn human() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.simon".into(),
    }
}
fn command(sequence: u64, revision: u64, id: &str, change: TaskChange) -> TaskControlCommand {
    TaskControlCommand {
        sequence,
        expected_revision: revision,
        command_id: id.into(),
        operator: human(),
        reason: "operator controls project work".into(),
        change,
    }
}
fn pause(runtime: &mut TaskRuntime, host: &mut Host, seq: u64, id: &str) {
    runtime
        .control_task(host, command(seq, 0, id, TaskChange::Pause))
        .unwrap();
}
fn resume(runtime: &mut TaskRuntime, host: &mut Host, seq: u64, id: &str) {
    runtime
        .control_task(
            host,
            command(
                seq,
                1,
                id,
                TaskChange::Resume {
                    recover_dispatched: false,
                },
            ),
        )
        .unwrap();
}
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    condition: impl Fn(&Host, &TaskRuntime) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(host, runtime) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn db(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap()
}

#[test]
fn a_paused_worker_is_collected_and_frees_its_session_for_unrelated_work() {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.route("main", "a");
    f.route("other", "a");
    let mut host_config = f.read("host.json");
    host_config["resource_pools"] = json!({"compute":1});
    host_config["adapters"][1]["reasoning_resources"] = json!({"compute":1});
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["routes"][1]["activation"] = json!({"kind":"at_time","unix_ms":100});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let end = Instant::now() + Duration::from_secs(30);
    while !f.started("a") {
        runtime.tick_at(&mut host, 0).unwrap();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    pause(&mut runtime, &mut host, 1, "pause-main");
    runtime.tick_at(&mut host, 100).unwrap();
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(host.resource_usage()[0].used, 1);
    f.release("a");
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(runtime.completed_after(0, 10).unwrap()[0].route_id, "other");
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 1);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(host.resource_usage()[0].used, 0);
    assert_eq!(
        plan_support::artifacts(&host, "arcs.solution_candidate.nearest.v1").len(),
        2
    );
    resume(&mut runtime, &mut host, 1, "resume-main");
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(
        f.count("a", "attempts"),
        2,
        "cached result was dispatched again"
    );
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 0);
    host.shutdown().unwrap();
}

#[test]
fn a_paused_planner_keeps_its_cached_proposal_over_restart_without_expansion() {
    let f = plan_support::fixture("normal", "normal");
    let mut profile = f.read("planner.json");
    profile["gate"] = json!(true);
    f.write("planner.json", profile);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| f.started("planner"));
    pause(&mut runtime, &mut host, 1, "pause-plan");
    f.release("planner");
    until(&mut host, &mut runtime, |host, _| {
        !plan_support::artifacts(host, "arcs.plan_candidate.v1").is_empty()
    });
    assert!(runtime.plans_after(0, 10).unwrap().is_empty());
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    assert!(!f.started("a") && !f.started("b"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert!(runtime.plans_after(0, 10).unwrap().is_empty());
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 1);
    resume(&mut runtime, &mut host, 1, "resume-plan");
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn parent_and_child_pauses_are_independent_and_a_join_cannot_pass_either() {
    let f = plan_support::fixture("gate", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| {
        f.started("a") && f.started("b")
    });
    let plan = runtime.plans_after(0, 10).unwrap().remove(0);
    let left = plan
        .steps
        .iter()
        .find(|step| step.id == "left")
        .unwrap()
        .sequence;
    pause(&mut runtime, &mut host, plan.sequence, "pause-parent");
    pause(&mut runtime, &mut host, left, "pause-child");
    f.release("a");
    f.release("b");
    until(&mut host, &mut runtime, |host, _| {
        plan_support::artifacts(host, "arcs.solution_candidate.nearest.v1").len() == 2
    });
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 4);
    assert!(!f.started("join"));
    resume(&mut runtime, &mut host, plan.sequence, "resume-parent");
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert!(!f.started("join"));
    let status = runtime
        .task_controls_after(0, 20)
        .unwrap()
        .into_iter()
        .find(|s| s.sequence == left)
        .unwrap();
    assert_eq!(status.paused_by, vec![left]);
    resume(&mut runtime, &mut host, left, "resume-child");
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(
        runtime.plans_after(0, 10).unwrap()[0].plan_version_id,
        plan.plan_version_id
    );
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn command_revisions_retries_and_a_failed_control_commit_do_not_discard_work() {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.route("main", "a");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| f.started("a"));
    let mut invalid = command(1, 0, "model-command", TaskChange::Pause);
    invalid.operator.actor_type = ActorType::Model;
    assert!(runtime.control_task(&mut host, invalid).is_err());
    db(&f).execute_batch("CREATE TRIGGER fail_control BEFORE INSERT ON runtime_task_control_events BEGIN SELECT RAISE(ABORT,'control checkpoint failed'); END;").unwrap();
    assert!(
        runtime
            .control_task(&mut host, command(1, 0, "pause", TaskChange::Pause))
            .is_err()
    );
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 0);
    assert!(runtime.task_controls_after(0, 10).unwrap().is_empty());
    db(&f).execute_batch("DROP TRIGGER fail_control").unwrap();
    let first = runtime
        .control_task(&mut host, command(1, 0, "pause", TaskChange::Pause))
        .unwrap();
    let replay = runtime
        .control_task(&mut host, command(1, 0, "pause", TaskChange::Pause))
        .unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    assert!(
        runtime
            .control_task(
                &mut host,
                command(
                    1,
                    0,
                    "stale",
                    TaskChange::Resume {
                        recover_dispatched: false
                    }
                )
            )
            .is_err()
    );
    let mut changed = command(1, 0, "pause", TaskChange::Pause);
    changed.reason = "different".into();
    assert!(runtime.control_task(&mut host, changed).is_err());
    resume(&mut runtime, &mut host, 1, "resume");
    runtime
        .control_task(&mut host, command(1, 0, "pause", TaskChange::Pause))
        .unwrap();
    assert_eq!(
        runtime.report(0).unwrap().paused_jobs,
        0,
        "old retry re-applied a stale pause"
    );
    let count: i64 = db(&f)
        .query_row(
            "SELECT COUNT(*) FROM runtime_task_control_events",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let history = runtime.task_control_history(1, 0, 10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].command.operator.id, "operator.simon");
    assert_eq!(runtime.task_control_history(1, 1, 10).unwrap().len(), 1);
    f.release("a");
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn lifetime_expiry_still_cancels_a_paused_task_before_it_can_dispatch() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("main", "a");
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":1000});
    config["routes"][0]["max_lifetime_ms"] = json!(10);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    runtime.tick_at(&mut host, 0).unwrap();
    pause(&mut runtime, &mut host, 1, "pause");
    runtime.tick_at(&mut host, 1000).unwrap();
    let report = runtime.report(0).unwrap();
    assert_eq!(report.expired_jobs, 1);
    assert_eq!(report.paused_jobs, 0);
    assert_eq!(f.count("a", "attempts"), 0);
    assert!(
        runtime
            .control_task(
                &mut host,
                command(
                    1,
                    1,
                    "resume",
                    TaskChange::Resume {
                        recover_dispatched: false
                    }
                )
            )
            .is_err()
    );
    host.shutdown().unwrap();
}

#[test]
fn cancelling_a_plan_step_leaves_its_sibling_running_until_the_plan_root_is_cancelled() {
    let f = plan_support::fixture("gate", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| {
        f.started("a") && f.started("b")
    });
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    let left = plan.steps.iter().find(|step| step.id == "left").unwrap();
    let cancel = command(left.sequence, 0, "cancel-left", TaskChange::Cancel);
    assert!(runtime.control_task(&mut host, cancel).unwrap().cancelled);
    let partial = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(partial.state, plan.state);
    assert_eq!(
        partial.steps.iter().find(|s| s.id == "left").unwrap().state,
        "cancelled"
    );
    assert_eq!(
        partial
            .steps
            .iter()
            .find(|s| s.id == "right")
            .unwrap()
            .state,
        "running"
    );
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 1);

    let cancel = command(plan.sequence, 0, "cancel-root", TaskChange::Cancel);
    let receipt = runtime.control_task(&mut host, cancel).unwrap();
    let stopped = runtime.plans_after(0, 1).unwrap().remove(0);
    assert!(receipt.cancelled);
    assert_eq!(
        stopped.state,
        arcs_host::task_runtime::planning::PlanState::Cancelled
    );
    assert_eq!(
        stopped.cancelled_at_unix_ms,
        Some(receipt.changed_at_unix_ms)
    );
    assert_eq!(stopped.plan_version_id, plan.plan_version_id);
    assert!(stopped.steps.iter().all(|step| step.state == "cancelled"));
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    f.release("a");
    f.release("b");
    host.shutdown().unwrap();
    assert!(!f.started("join"));
}

#[test]
fn paused_collection_checks_an_expiry_that_the_bounded_sweep_has_not_reached() {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.peer("b", "gate", false);
    f.route("earlier", "b");
    f.route("paused", "a");
    let mut config = f.read("runtime.json");
    config["max_jobs_per_tick"] = json!(1);
    for route in config["routes"].as_array_mut().unwrap() {
        route["max_lifetime_ms"] = json!(10);
    }
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let end = Instant::now() + Duration::from_secs(30);
    while !(f.started("a") && f.started("b")) {
        runtime.tick_at(&mut host, 0).unwrap();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    pause(&mut runtime, &mut host, 2, "pause");
    f.release("a");
    let tick = runtime.tick_at(&mut host, 10).unwrap();
    assert!(tick.expired_tasks.contains(&1) && tick.expired_tasks.contains(&2));
    assert!(plan_support::artifacts(&host, "arcs.solution_candidate.nearest.v1").is_empty());
    f.release("b");
    host.shutdown().unwrap();
}

fn goal(runtime: &TaskRuntime) -> GoalTask {
    runtime.goals_after(0, 10).unwrap().remove(0)
}
fn goal_until(host: &mut Host, runtime: &mut TaskRuntime, state: GoalState) -> GoalTask {
    until(host, runtime, |_, r| {
        r.goals_after(0, 10)
            .unwrap()
            .first()
            .is_some_and(|goal| goal.state == state)
    });
    goal(runtime)
}
fn actuations(f: &goal_support::Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap()
}
fn approve(host: &Host, runtime: &mut TaskRuntime, task: &GoalTask) {
    runtime
        .review_goal(
            host,
            task.sequence,
            task.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "reviewed current evidence".into(),
        )
        .unwrap();
}

#[test]
fn task_cancellation_retains_an_unresolved_simulated_action_reservation() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device.fan-01"]);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let waiting = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    approve(&host, &mut runtime, &waiting);
    let dispatched = goal_until(&mut host, &mut runtime, GoalState::Observing);
    let reservation = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert_eq!(reservation.state, ReservationState::Held);
    assert!(dispatched.dispatch_cursor.is_some());
    assert_eq!(actuations(&f), 1);
    pause(&mut runtime, &mut host, waiting.sequence, "pause-action");
    let cancel = command(waiting.sequence, 1, "cancel-action", TaskChange::Cancel);
    let receipt = runtime.control_task(&mut host, cancel).unwrap();
    assert!(receipt.cancelled);
    assert!(!receipt.paused);
    let stopped = goal(&runtime);
    assert_eq!(stopped.state, GoalState::Cancelled);
    assert_eq!(stopped.dispatch_cursor, dispatched.dispatch_cursor);
    assert_eq!(stopped.execution, dispatched.execution);
    host.shutdown().unwrap();
    drop((runtime, host));

    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick(&mut host).unwrap();
    let held = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert_eq!(held.version_id, reservation.version_id);
    assert_eq!(held.state, reservation.state);
    assert_eq!(goal(&runtime).state, GoalState::Cancelled);
    assert_eq!(actuations(&f), 1);
    host.shutdown().unwrap();
}

#[test]
fn resuming_an_approved_goal_reacquires_replans_and_requires_a_new_approval() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let old = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    approve(&host, &mut runtime, &old);
    pause(&mut runtime, &mut host, old.sequence, "pause");
    assert!(
        runtime
            .review_goal(
                &host,
                old.sequence,
                old.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "stale".into()
            )
            .is_err()
    );
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(actuations(&f), 0);
    resume(&mut runtime, &mut host, old.sequence, "resume");
    let fresh = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    assert_eq!(fresh.round, old.round + 1);
    assert_ne!(fresh.before, old.before);
    assert_ne!(fresh.candidate, old.candidate);
    assert!(fresh.approval.is_none());
    assert!(
        runtime
            .review_goal(
                &host,
                old.sequence,
                old.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "old candidate".into()
            )
            .is_err()
    );
    approve(&host, &mut runtime, &fresh);
    until(&mut host, &mut runtime, |_, r| {
        matches!(
            goal(r).state,
            GoalState::AwaitingApproval | GoalState::Completed
        )
    });
    if goal(&runtime).state == GoalState::AwaitingApproval {
        let task = goal(&runtime);
        approve(&host, &mut runtime, &task);
    }
    goal_until(&mut host, &mut runtime, GoalState::Completed);
    assert_eq!(actuations(&f), 2);
    host.shutdown().unwrap();
}

#[test]
fn restarting_a_pre_action_goal_does_not_expand_its_round_budget() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["max_rounds"] = json!(1);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let old = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    pause(&mut runtime, &mut host, old.sequence, "pause");
    resume(&mut runtime, &mut host, old.sequence, "resume");
    assert_eq!(goal(&runtime).state, GoalState::BudgetExhausted);
    assert_eq!(actuations(&f), 0);
    host.shutdown().unwrap();
}

#[test]
fn planning_after_a_previous_effect_is_a_new_attempt_even_with_old_dispatch_references() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    approve(&host, &mut runtime, &first);
    let planning = goal_until(&mut host, &mut runtime, GoalState::Planning);
    assert_eq!(planning.round, 2);
    assert!(planning.dispatch_cursor.is_some());
    assert_eq!(actuations(&f), 1);
    pause(
        &mut runtime,
        &mut host,
        planning.sequence,
        "pause-next-plan",
    );
    resume(
        &mut runtime,
        &mut host,
        planning.sequence,
        "resume-next-plan",
    );
    let next = goal(&runtime);
    assert_eq!(next.round, 3);
    assert_eq!(next.state, GoalState::Acquiring);
    assert!(next.dispatch_cursor.is_none());
    let fresh = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    assert_ne!(fresh.before, planning.after);
    approve(&host, &mut runtime, &fresh);
    goal_until(&mut host, &mut runtime, GoalState::Completed);
    assert_eq!(actuations(&f), 2);
    host.shutdown().unwrap();
}

#[test]
fn a_parent_plan_resume_refreshes_its_goal_child_without_granting_action_authority() {
    let f = goal_support::plan_fixture();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let old = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    let plan = runtime.plans_after(0, 10).unwrap().remove(0);
    pause(&mut runtime, &mut host, plan.sequence, "pause-plan");
    assert!(
        runtime
            .review_goal(
                &host,
                old.sequence,
                old.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "paused child".into()
            )
            .is_err()
    );
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(actuations(&f), 0);
    resume(&mut runtime, &mut host, plan.sequence, "resume-plan");
    let fresh = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    assert_eq!(fresh.round, 2);
    assert_ne!(fresh.candidate, old.candidate);
    approve(&host, &mut runtime, &fresh);
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(actuations(&f), 1);
    assert_eq!(
        runtime.plans_after(0, 10).unwrap()[0].plan_version_id,
        plan.plan_version_id
    );
    host.shutdown().unwrap();
}

#[test]
fn unresolved_dispatch_resume_is_explicit_and_reuses_the_existing_device_effect() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut profile = f.read("input-profile.json");
    profile["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", profile);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let old = goal_until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    approve(&host, &mut runtime, &old);
    f.db("runtime").execute_batch("CREATE TRIGGER lost_execution_checkpoint BEFORE UPDATE OF goal_json ON runtime_jobs WHEN json_extract(NEW.goal_json,'$.state')='observing' BEGIN SELECT RAISE(ABORT,'lost checkpoint'); END;").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let error = loop {
        match runtime.tick(&mut host) {
            Ok(_) => {
                assert!(
                    Instant::now() < deadline,
                    "execution checkpoint was never attempted"
                );
                std::thread::sleep(Duration::from_millis(3));
            }
            Err(error) => break error,
        }
    };
    assert!(error.to_string().contains("lost checkpoint"), "{error}");
    assert_eq!(actuations(&f), 1);
    f.db("runtime")
        .execute_batch("DROP TRIGGER lost_execution_checkpoint")
        .unwrap();
    let uncertain = goal(&runtime);
    assert_eq!(uncertain.state, GoalState::Executing);
    assert!(uncertain.dispatch_cursor.is_some());
    pause(&mut runtime, &mut host, old.sequence, "pause");
    assert!(runtime.task_controls_after(0, 10).unwrap()[0].action_recovery_required);
    assert!(
        runtime
            .control_task(
                &mut host,
                command(
                    old.sequence,
                    1,
                    "resume-without-recovery",
                    TaskChange::Resume {
                        recover_dispatched: false
                    }
                )
            )
            .is_err()
    );
    assert_eq!(runtime.task_controls_after(0, 10).unwrap()[0].revision, 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime
        .control_task(
            &mut host,
            command(
                old.sequence,
                1,
                "recover",
                TaskChange::Resume {
                    recover_dispatched: true,
                },
            ),
        )
        .unwrap();
    goal_until(&mut host, &mut runtime, GoalState::Completed);
    assert_eq!(actuations(&f), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_failed_paused_response_survives_a_lost_queue_update_and_is_not_implicitly_retried() {
    let f = Fixture::new();
    f.peer("a", "gate", false);
    f.route("main", "a");
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path,source.replace(" elif kind=='poll':", "   reply={'type':'error','code':'unavailable','message':'failed after gate','retryable':True}\n elif kind=='poll':")).unwrap();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| f.started("a"));
    pause(&mut runtime, &mut host, 1, "pause");
    db(&f).execute_batch("CREATE TRIGGER lost_failure BEFORE UPDATE ON runtime_jobs WHEN NEW.state='failed' BEGIN SELECT RAISE(ABORT,'lost failure checkpoint'); END;").unwrap();
    f.release("a");
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    db(&f).execute_batch("DROP TRIGGER lost_failure").unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    resume(&mut runtime, &mut host, 1, "resume");
    let tick = runtime.tick(&mut host).unwrap();
    assert_eq!(tick.failed.len(), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert!(
        tick.failed[0]
            .error
            .contains("paused reasoning invocation failed")
    );
    host.shutdown().unwrap();
}

#[test]
fn a_goal_paused_before_its_first_checkpoint_uses_a_fresh_barrier_without_spending_an_attempt() {
    let f = goal_support::Fixture::new();
    f.manual();
    f.with_observer(false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":1000});
    config["routes"][0]["goal"]["max_rounds"] = json!(1);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    host.poll_inputs().unwrap();
    runtime.tick_at(&mut host, 0).unwrap();
    assert!(runtime.goals_after(0, 10).unwrap().is_empty());
    pause(&mut runtime, &mut host, 1, "pause");
    for _ in 0..3 {
        runtime.tick_at(&mut host, 500).unwrap();
    }
    let cursor = host.store().latest_sequence().unwrap();
    resume(&mut runtime, &mut host, 1, "resume");
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick_at(&mut host, 1000).unwrap();
        if goal(&runtime).state == GoalState::AwaitingApproval {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let task = goal(&runtime);
    assert_eq!(task.round, 1);
    let events = host.store().committed_after(cursor).unwrap();
    assert!(task.before.iter().all(|version| {
        events
            .iter()
            .any(|event| &event.artifact.version_id == version)
    }));
    assert_eq!(actuations(&f), 0);
    host.shutdown().unwrap();
}

#[test]
fn cli_commands_and_additive_migration_preserve_task_control_across_restarts() {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("main", "a");
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    runtime.tick_at(&mut host, 0).unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db(&f).execute_batch("DROP TABLE runtime_task_control_events;DROP TABLE runtime_task_controls;DROP TABLE runtime_task_resume_barriers;ALTER TABLE runtime_reasoning_requests DROP COLUMN pause_captured;ALTER TABLE runtime_reasoning_requests DROP COLUMN abandoned;").unwrap();
    let run = |file: &str| {
        let result = std::process::Command::new(support::adapter_binary("arcs-host"))
            .args(["--config"])
            .arg(f.directory.join("host.json"))
            .arg("--runtime")
            .arg(f.directory.join("runtime.json"))
            .arg("--task-command")
            .arg(f.directory.join(file))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()
    };
    f.write(
        "pause.json",
        serde_json::to_value(command(1, 0, "pause", TaskChange::Pause)).unwrap(),
    );
    assert_eq!(run("pause.json")["paused_jobs"], 1);
    assert_eq!(run("pause.json")["task_controls"][0]["revision"], 1);
    let (mut host, mut runtime) = f.open();
    for _ in 0..3 {
        runtime.tick_at(&mut host, 500).unwrap();
    }
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.write(
        "resume.json",
        serde_json::to_value(command(
            1,
            1,
            "resume",
            TaskChange::Resume {
                recover_dispatched: false,
            },
        ))
        .unwrap(),
    );
    assert_eq!(run("resume.json")["paused_jobs"], 0);
    f.write(
        "cancel.json",
        json!(command(1, 2, "cancel", TaskChange::Cancel)),
    );
    let stopped = run("cancel.json");
    assert_eq!(stopped["cancelled_jobs"], 1);
    assert_eq!(stopped["task_control"]["cancelled"], true);
    assert_eq!(stopped["task_control"]["revision"], 3);
    assert_eq!(run("cancel.json")["task_control"], stopped["task_control"]);
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 1);
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 0);
    assert_eq!(f.count("a", "attempts"), 0);
    host.shutdown().unwrap();
}

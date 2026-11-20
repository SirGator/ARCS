#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs_host::task_runtime::planning::PlanState;
use plan_support::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[test]
fn cancelling_active_branches_discards_late_answers_and_reuses_the_session_for_unrelated_work() {
    let f = fixture("gate", "gate");
    f.route("independent", "a");
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(5);
    config["routes"][1]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    let original = runtime.plans_after(0, 10).unwrap().remove(0);
    let plan_version = original.plan_version_id.clone().unwrap();
    runtime
        .cancel_plan(&mut host, original.sequence, "project withdrawn".into())
        .unwrap();
    let receipt = runtime.plans_after(0, 10).unwrap().remove(0);
    assert_eq!(receipt.state, PlanState::Cancelled);
    assert_eq!(receipt.plan_version_id.as_ref(), Some(&plan_version));
    assert!(receipt.steps.iter().all(|s| s.state == "cancelled"));
    assert_eq!(receipt.detail.as_deref(), Some("project withdrawn"));
    assert!(receipt.cancelled_at_unix_ms.is_some());
    assert_eq!(runtime.report(0).unwrap().capacity_used, 1);
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 4);
    assert_eq!(runtime.report(0).unwrap().blocked_goals, 0);
    assert_eq!(runtime.report(0).unwrap().running_plans, 0);
    assert_eq!(runtime.report(0).unwrap().stopped_plans, 1);
    // Both cancelled workers still occupy the two worker slots. Neither a
    // shared session nor the global worker limit may be reused prematurely.
    runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    f.release("a");
    f.release("b");
    let deadline = Instant::now() + Duration::from_secs(20);
    while runtime.report(0).unwrap().ready_for_review != 1 {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let completed = runtime.completed_after(0, 20).unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].route_id, "independent");
    assert!(
        runtime
            .cancel_plan(&mut host, completed[0].sequence, "ordinary route".into())
            .is_err()
    );
    assert_eq!(
        artifacts(&host, "arcs.solution_candidate.nearest.v1").len(),
        1
    );
    assert_eq!(f.count("a", "attempts"), 2);
    assert!(!f.started("join"));
    let snapshot = host.store().get(&plan_version).unwrap().unwrap();
    assert_eq!(snapshot.payload["steps"].as_array().unwrap().len(), 3);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..6 {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    }
    assert_eq!(
        runtime.plans_after(0, 10).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 2);
    assert!(!f.started("join"));
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn a_cancelled_planner_never_expands_after_a_lost_answer_or_restart() {
    let f = fixture("normal", "normal");
    let mut model = f.read("planner.json");
    model["gate"] = json!(true);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| f.started("planner"));
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    runtime
        .cancel_plan(&mut host, 1, "stop planning".into())
        .unwrap();
    let first = serde_json::to_value(runtime.plans_after(0, 10).unwrap()).unwrap();
    runtime
        .cancel_plan(&mut host, 1, "stop planning".into())
        .unwrap();
    assert_eq!(
        serde_json::to_value(runtime.plans_after(0, 10).unwrap()).unwrap(),
        first
    );
    assert!(
        runtime
            .cancel_plan(&mut host, 1, "replace reason".into())
            .is_err()
    );
    let receipt = runtime.plans_after(0, 10).unwrap().remove(0);
    assert_eq!(receipt.state, PlanState::Cancelled);
    assert!(receipt.steps.is_empty() && receipt.plan_version_id.is_none());
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("planner");
    let (mut host, mut runtime) = f.open();
    for _ in 0..8 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(f.count("planner", "attempts"), 1);
    assert!(artifacts(&host, "arcs.plan.v1").is_empty());
    assert!(artifacts(&host, "arcs.plan_candidate.v1").is_empty());
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 1);
    assert!(!f.started("a") && !f.started("b") && !f.started("join"));
    host.shutdown().unwrap();
}

#[test]
fn cancellation_keeps_finished_results_and_does_not_wake_monthly_steps() {
    let f = fixture("normal", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["activation"] =
        json!({"kind":"after_delay","delay_ms":2_592_000_000u64});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    let completed = runtime.completed_after(0, 10).unwrap();
    let original = runtime.plans_after(0, 1).unwrap().remove(0);
    runtime
        .cancel_plan(&mut host, original.sequence, "stop waiting".into())
        .unwrap();
    let cancelled = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(
        cancelled
            .steps
            .iter()
            .filter(|s| s.state == "ready_for_review")
            .count(),
        1
    );
    assert_eq!(
        cancelled
            .steps
            .iter()
            .filter(|s| s.state == "cancelled")
            .count(),
        2
    );
    assert_eq!(runtime.report(0).unwrap().waiting_time, 0);
    assert_eq!(runtime.report(0).unwrap().waiting_dependency, 0);
    assert_eq!(runtime.completed_after(0, 10).unwrap(), completed);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..6 {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    }
    assert!(!f.started("a") && !f.started("join"));
    assert_eq!(runtime.completed_after(0, 10).unwrap(), completed);
    assert_eq!(f.count("planner", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_failed_cancellation_commit_neither_changes_jobs_nor_discards_live_answers() {
    let f = fixture("gate", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    journal.execute_batch("CREATE TRIGGER fail_cancel BEFORE UPDATE OF state ON runtime_jobs WHEN NEW.state='cancelled' AND OLD.sequence>1 BEGIN SELECT RAISE(ABORT,'injected cancellation failure'); END;").unwrap();
    assert!(
        runtime
            .cancel_plan(&mut host, 1, "will fail".into())
            .is_err()
    );
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Running
    );
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 0);
    assert_eq!(
        journal
            .query_row("SELECT COUNT(*) FROM runtime_plan_cancellations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    journal.execute_batch("DROP TRIGGER fail_cancel").unwrap();
    f.release("a");
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert!(
        runtime
            .cancel_plan(&mut host, 1, "too late".into())
            .is_err()
    );
    host.shutdown().unwrap();
}

#[test]
fn cli_cancels_a_sleeping_planner_and_rejects_conflicting_commands_before_opening_the_core() {
    let f = fixture("normal", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    runtime.tick(&mut host).unwrap();
    assert_eq!(runtime.planning_jobs_after(0, 1).unwrap()[0].sequence, 1);
    assert_eq!(runtime.report(0).unwrap().planning_jobs[0].state, "pending");
    assert!(runtime.planning_jobs_after(0, 0).is_err());
    assert!(runtime.planning_jobs_after(0, 1001).is_err());
    for (sequence, reason) in [(0, "stop"), (u64::MAX, "stop"), (55, "stop"), (1, " ")] {
        assert!(
            runtime
                .cancel_plan(&mut host, sequence, reason.into())
                .is_err()
        );
    }
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    // Simulate a journal from before plan cancellation was introduced. Its
    // existing identity and queued planner must survive the additive upgrade.
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    journal
        .execute_batch("DROP TABLE runtime_plan_cancellations")
        .unwrap();
    drop(journal);
    f.write(
        "cancel.json",
        json!({"command":"cancel","sequence":1,"reason":"operator cancelled pending plan"}),
    );
    let run = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .current_dir(&f.directory)
            .args(["--config", "host.json", "--runtime", "runtime.json"])
            .args(extra)
            .output()
            .unwrap()
    };
    let out = run(&["--plan-command", "cancel.json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["plans"][0]["state"], "cancelled");
    assert_eq!(report["capacity_used"], 0);
    assert_eq!(report["cancelled_jobs"], 1);
    assert_eq!(report["planning_jobs"], json!([]));
    assert!(!f.started("planner"));
    let core = std::fs::read(f.directory.join("core.sqlite")).unwrap();
    for args in [
        vec![
            "--plan-command",
            "cancel.json",
            "--goal-command",
            "cancel.json",
        ],
        vec!["--plan-command", "cancel.json", "--ticks", "1"],
    ] {
        assert!(!run(&args).status.success());
        assert_eq!(
            std::fs::read(f.directory.join("core.sqlite")).unwrap(),
            core
        );
    }
}

#[test]
fn cancelling_one_input_plan_does_not_cancel_another_using_the_same_planner() {
    let f = fixture("normal", "normal");
    let mut input = f.read("input-profile.json");
    input["events"].as_array_mut().unwrap().push(json!({"id":"second","subject":"second-project","reference":"task:second","payload":{"values":[3,7],"target":6}}));
    f.write("input-profile.json", input);
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(8);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["gate"] = json!(true);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("planner") && r.report(0).unwrap().pending_jobs == 2
    });
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 2);
    runtime
        .cancel_plan(&mut host, 1, "only the first input".into())
        .unwrap();
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    f.release("planner");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    let plans = runtime.plans_after(0, 10).unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].state, PlanState::Cancelled);
    assert!(plans[0].steps.is_empty());
    assert_eq!(plans[1].state, PlanState::ReadyForReview);
    assert_ne!(plans[0].input_version_id, plans[1].input_version_id);
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 4);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

#[test]
fn cancelled_stalled_workers_release_global_capacity_at_their_original_deadlines() {
    let f = fixture("gate", "gate");
    f.peer("other", "normal", false);
    f.route("independent", "other");
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(5);
    config["routes"][1]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    for index in [0, 1] {
        config["routes"][0]["plan"]["templates"][index]["route"]["budget"]["max_runtime_ms"] =
            json!(2000);
    }
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    runtime
        .cancel_plan(&mut host, 1, "stop stalled plan".into())
        .unwrap();
    runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    assert!(!f.started("other"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while runtime.report(0).unwrap().ready_for_review != 1 {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(f.started("other"));
    assert!(!f.started("join"));
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 0);
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 4);
    assert_eq!(
        artifacts(&host, "arcs.solution_candidate.nearest.v1").len(),
        1
    );
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    host.shutdown().unwrap();
}

#[test]
fn child_jobs_foreign_cores_and_finished_plans_cannot_be_cancelled_through_plan_control() {
    let f = fixture("gate", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert!(
        runtime
            .cancel_plan(&mut host, plan.steps[0].sequence, "wrong scope".into())
            .is_err()
    );
    assert!(
        runtime
            .cancel_plan(&mut host, plan.sequence, "a".repeat(2001))
            .is_err()
    );
    let foreign = fixture("normal", "normal");
    let (mut other, other_runtime) = foreign.open();
    assert!(
        runtime
            .cancel_plan(&mut other, plan.sequence, "wrong core".into())
            .is_err()
    );
    other.shutdown().unwrap();
    drop(other_runtime);
    assert_eq!(runtime.report(0).unwrap().cancelled_jobs, 0);
    f.release("a");
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert!(
        runtime
            .cancel_plan(&mut host, plan.sequence, "already finished".into())
            .is_err()
    );
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 4);
    host.shutdown().unwrap();
}

#[test]
fn a_blocked_plan_can_still_cancel_its_remaining_parallel_work() {
    let f = fixture("invalid", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("b") && r.report(0).unwrap().stopped_plans == 1
    });
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Blocked
    );
    runtime
        .cancel_plan(&mut host, 1, "stop remaining work after failure".into())
        .unwrap();
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    f.release("b");
    for _ in 0..8 {
        runtime.tick(&mut host).unwrap();
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 0);
    assert!(artifacts(&host, "arcs.solution_candidate.nearest.v1").is_empty());
    assert!(!f.started("join"));
    assert_eq!(runtime.report(0).unwrap().stopped_plans, 1);
    host.shutdown().unwrap();
}

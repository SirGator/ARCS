#[path = "support/goals.rs"]
mod goal_support;
mod support;
use arcs::{
    approval::ApprovalDecision,
    core::{Actor, ActorType},
    runtime::reservations::ReservationState,
};
use arcs_host::{
    Host,
    task_runtime::{
        RuntimeConfig, TaskRuntime,
        goal::{GoalState, GoalTask},
    },
};
use goal_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn fixture(two: bool) -> Fixture {
    let f = Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(2);
    config["max_jobs_per_tick"] = json!(10);
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device.fan-01"]);
    config["routes"][0]["goal"]["max_rounds"] = json!(1);
    if two {
        let mut second = config["routes"][0].clone();
        second["id"] = json!("second");
        second["goal"]["initial_assessment"] = json!(true);
        config["routes"].as_array_mut().unwrap().push(second);
    }
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", input);
    f
}
fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(runtime) {
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
fn waiting(host: &mut Host, runtime: &mut TaskRuntime) -> GoalTask {
    until(host, runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.state == GoalState::AwaitingApproval)
    });
    runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.state == GoalState::AwaitingApproval)
        .unwrap()
}
fn actor() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.simon".into(),
    }
}
fn approve(host: &Host, runtime: &mut TaskRuntime, goal: &GoalTask) {
    runtime
        .review_goal(
            host,
            goal.sequence,
            goal.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            actor(),
            "reviewed simulated device action".into(),
        )
        .unwrap();
}
fn acts(f: &Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn competing_goals_acquire_exclusively_before_reading_state_and_release_after_assessment() {
    let f = fixture(true);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = waiting(&mut host, &mut runtime);
    assert_eq!(runtime.goals_after(0, 10).unwrap().len(), 1);
    assert_eq!(first.route_id, "fan-cycle");
    let held = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert_eq!(held.state, ReservationState::Held);
    assert_eq!(held.owner.task_sequence, first.sequence);
    let tick = runtime.tick(&mut host).unwrap();
    assert!(
        tick.waiting_resources
            .iter()
            .any(|w| w.route_id == "second" && w.resource == "device.fan-01")
    );
    let open: i64 = f
        .db("runtime")
        .query_row(
            "SELECT COUNT(*) FROM runtime_control_sessions WHERE sequence=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 1);
    approve(&host, &mut runtime, &first);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().completed_goals == 2
    });
    assert_eq!(acts(&f), 1);
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    let second = runtime.goals_after(1, 10).unwrap().remove(0);
    let before = host.store().get(&second.before[0]).unwrap().unwrap();
    assert_eq!(before.payload["actuation_count"], 1);
    host.shutdown().unwrap();
}

#[test]
fn a_restart_keeps_the_same_owner_and_revision_until_the_goal_finishes() {
    let f = fixture(false);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = waiting(&mut host, &mut runtime);
    let reservation = runtime.exclusive_resources(&host).unwrap().remove(0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].version_id,
        reservation.version_id
    );
    approve(&host, &mut runtime, &goal);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().completed_goals == 1
    });
    assert_eq!(acts(&f), 1);
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    host.shutdown().unwrap();
}

#[test]
fn rejection_before_dispatch_releases_but_stale_reconciliation_cannot_release_the_successor() {
    let f = fixture(true);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = waiting(&mut host, &mut runtime);
    let old = runtime.exclusive_resources(&host).unwrap().remove(0);
    runtime
        .review_goal(
            &host,
            first.sequence,
            first.candidate.as_ref().unwrap(),
            ApprovalDecision::Rejected,
            actor(),
            "reject before action".into(),
        )
        .unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    let second = waiting(&mut host, &mut runtime);
    assert_eq!(second.route_id, "second");
    assert!(
        runtime
            .reconcile_goal_resources(
                &host,
                first.sequence,
                &[old.version_id],
                std::slice::from_ref(&first.before[0]),
                actor(),
                "stale acknowledgement".into()
            )
            .is_err()
    );
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0]
            .owner
            .task_sequence,
        second.sequence
    );
    assert_eq!(acts(&f), 0);
    runtime
        .cancel_goal(&host, second.sequence, "done".into())
        .unwrap();
    host.shutdown().unwrap();
}

#[test]
fn cancellation_after_dispatch_holds_until_explicit_terminal_reconciliation_and_cli_retry_is_idempotent()
 {
    let f = fixture(false);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = waiting(&mut host, &mut runtime);
    approve(&host, &mut runtime, &goal);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10).unwrap()[0].state == GoalState::Observing
    });
    assert_eq!(acts(&f), 1);
    let held = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert!(
        runtime
            .reconcile_goal_resources(
                &host,
                goal.sequence,
                std::slice::from_ref(&held.version_id),
                std::slice::from_ref(&goal.before[0]),
                actor(),
                "still active".into()
            )
            .is_err()
    );
    runtime
        .cancel_goal(&host, goal.sequence, "stop before assessment".into())
        .unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Held
    );
    host.poll_round().unwrap();
    let evidence = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .rev()
        .find(|e| e.artifact.schema_id.0 == "arcs.state.fan.v1")
        .unwrap()
        .artifact
        .version_id;
    assert!(
        runtime
            .reconcile_goal_resources(
                &host,
                goal.sequence,
                std::slice::from_ref(&held.version_id),
                &[],
                actor(),
                "missing evidence".into()
            )
            .is_err()
    );
    assert!(
        runtime
            .reconcile_goal_resources(
                &host,
                goal.sequence,
                std::slice::from_ref(&held.version_id),
                std::slice::from_ref(&evidence),
                Actor {
                    actor_type: ActorType::Model,
                    id: "model".into()
                },
                "model cannot release".into()
            )
            .is_err()
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Held
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.write("release.json",json!({"command":"release_resources","sequence":goal.sequence,"reservation_versions":[held.version_id],"evidence_versions":[evidence],"operator_id":"operator.simon","reason":"simulated device state checked; control may be handed over"}));
    let mut revision = None;
    for _ in 0..2 {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .current_dir(&f.directory)
            .args([
                "--config",
                "host.json",
                "--runtime",
                "runtime.json",
                "--goal-command",
                "release.json",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        let record = &value["resource_reservations"][0];
        assert_eq!(record["state"], "released");
        if let Some(expected) = &revision {
            assert_eq!(&record["version_id"], expected);
        }
        revision = Some(record["version_id"].clone());
    }
    assert_eq!(acts(&f), 1);
}

#[test]
fn separate_hosts_using_the_same_core_observe_the_same_reservation() {
    let f = fixture(false);
    let mut second = f.read("runtime.json");
    second["runtime_id"] = json!("other-controller");
    second["journal"] = json!("other.sqlite");
    second["routes"][0]["id"] = json!("other-goal");
    f.write("other-runtime.json", second);
    let mut one = f.host();
    let mut first = f.runtime(&one);
    let goal = waiting(&mut one, &mut first);
    let mut two = f.host();
    let mut other = TaskRuntime::open(
        &two,
        RuntimeConfig::load(f.directory.join("other-runtime.json")).unwrap(),
    )
    .unwrap();
    for _ in 0..4 {
        let tick = other.tick(&mut two).unwrap();
        assert!(!tick.waiting_resources.is_empty());
    }
    assert!(other.goals_after(0, 10).unwrap().is_empty());
    assert_eq!(
        other.exclusive_resources(&two).unwrap()[0].owner,
        first.exclusive_resources(&one).unwrap()[0].owner
    );
    first
        .cancel_goal(&one, goal.sequence, "handover before dispatch".into())
        .unwrap();
    let next = waiting(&mut two, &mut other);
    assert_eq!(next.route_id, "other-goal");
    approve(&two, &mut other, &next);
    until(&mut two, &mut other, |r| {
        r.report(0).unwrap().completed_goals == 1
    });
    assert_eq!(acts(&f), 1);
    one.shutdown().unwrap();
    two.shutdown().unwrap();
}

#[test]
fn a_monthly_wait_releases_control_and_waking_starts_a_fresh_session() {
    let f = fixture(false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["max_rounds"] = json!(2);
    config["routes"][0]["goal"]["between_rounds"] =
        json!({"kind":"after_delay","delay_ms":2_592_000_000u64});
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":false,"minimum_actuations":2});
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = waiting(&mut host, &mut runtime);
    let old = runtime.exclusive_resources(&host).unwrap().remove(0);
    approve(&host, &mut runtime, &first);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10).unwrap()[0].state == GoalState::WaitingTime
    });
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    let due = runtime.scheduled_after(0, 10).unwrap()[0]
        .not_before_unix_ms
        .unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick_at(&mut host, due - 1).unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick_at(&mut host, due).unwrap();
        if runtime.goals_after(0, 10).unwrap()[0].state == GoalState::AwaitingApproval {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let second = runtime.goals_after(0, 10).unwrap().remove(0);
    let fresh = runtime.exclusive_resources(&host).unwrap().remove(0);
    assert_ne!(fresh.owner, old.owner);
    assert_eq!(fresh.owner.session, 2);
    assert_ne!(second.candidate, first.candidate);
    approve(&host, &mut runtime, &second);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().completed_goals == 1
    });
    assert_eq!(acts(&f), 2);
    host.shutdown().unwrap();
}

#[test]
fn consecutive_rounds_keep_the_same_control_session_and_waiters_cannot_interleave() {
    let f = fixture(true);
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["max_rounds"] = json!(2);
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":false,"minimum_actuations":2});
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = waiting(&mut host, &mut runtime);
    let held = runtime.exclusive_resources(&host).unwrap().remove(0);
    approve(&host, &mut runtime, &first);
    let next = waiting(&mut host, &mut runtime);
    assert_eq!(next.sequence, first.sequence);
    assert_eq!(next.round, 2);
    assert_eq!(runtime.goals_after(0, 10).unwrap().len(), 1);
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].version_id,
        held.version_id
    );
    approve(&host, &mut runtime, &next);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().completed_goals == 2
    });
    assert_eq!(acts(&f), 2);
    host.shutdown().unwrap();
}

#[test]
fn a_dynamic_goal_snapshot_keeps_its_resources_and_plan_cancellation_closes_its_session() {
    let f = fixture(false);
    let mut config = f.read("runtime.json");
    let goal = config["routes"][0].clone();
    let mut root = goal.clone();
    root.as_object_mut().unwrap().remove("goal");
    root["id"] = json!("workflow");
    root["method"] = json!("learned");
    root["reasoner"]["adapter_id"] = json!("planner");
    root["target_schema_id"] = json!("arcs.plan_candidate.v1");
    root["allowed_capabilities"] = json!([]);
    root["plan"] = json!({"max_steps":1,"max_total_actions":1,"max_total_runtime_ms":30000,"templates":[{"route":goal,"export_fields":["goal_status","effect"]}]});
    config["routes"] = json!([root]);
    f.write("runtime.json", config);
    let script = include_str!("support/planner_peer.py");
    std::fs::write(f.directory.join("workflow.py"), script).unwrap();
    f.write(
        "workflow.json",
        json!({"journal_name":"root-workflow","proposal":{"steps":[{"id":"device","template":"fan-cycle"}]}}),
    );
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("planner");
    grant["producer_class"] = json!("model");
    grant["reasoning_limits_v2"]["methods"] = json!(["learned"]);
    f.write("root-grant.json", grant);
    let mut host_config = f.read("host.json");
    host_config["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"workflow.py"},{"path":"workflow.json"}],"grant_file":"root-grant.json","timeout_ms":15000}));
    f.write("host.json", host_config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = waiting(&mut host, &mut runtime);
    assert_eq!(goal.route_id, "p1.device");
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    let artifact = host
        .store()
        .get(plan.plan_version_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        artifact.payload["steps"][0]["route"]["goal"]["exclusive_resources"],
        json!(["device.fan-01"])
    );
    runtime
        .cancel_plan(&mut host, 1, "stop dynamic device work".into())
        .unwrap();
    assert_eq!(
        runtime.exclusive_resources(&host).unwrap()[0].state,
        ReservationState::Released
    );
    assert_eq!(
        runtime.goals_after(0, 10).unwrap()[0].state,
        GoalState::Cancelled
    );
    assert_eq!(acts(&f), 0);
    host.shutdown().unwrap();
}

#[test]
fn invalid_resource_names_and_external_reservation_publishers_are_rejected() {
    let f = fixture(false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device / invalid"]);
    f.write("runtime.json", config);
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
    let other = fixture(false);
    let mut grant = other.read("planner-grant.json");
    grant["adapter_id"] = json!("arcs.runtime.reservations");
    other.write("planner-grant.json", grant);
    assert!(Host::open(other.directory.join("host.json")).is_err());
    assert!(!other.directory.join("core.sqlite").exists());
}

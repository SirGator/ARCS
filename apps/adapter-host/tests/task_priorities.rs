#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use parallel_support::Fixture;
use serde_json::json;

fn reopen(f: &Fixture, host: &Host) -> TaskRuntime {
    TaskRuntime::open(
        host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    )
    .unwrap()
}

fn gated() -> Fixture {
    let f = Fixture::new();
    for name in ["a", "b"] {
        f.peer(name, "gate", false);
        f.route(name, name);
    }
    let mut config = f.read("runtime.json");
    config["max_jobs_per_tick"] = json!(1);
    f.write("runtime.json", config);
    f
}

#[test]
fn single_step_restarts_preserve_both_weights_and_low_priority_progress() {
    let f = gated();
    let mut config = f.read("runtime.json");
    config["routes"][0]["priority"] = json!(3);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    for _ in 0..18 {
        let tick = runtime.tick(&mut host).unwrap();
        assert!(tick.failed.is_empty());
        drop(runtime);
        runtime = reopen(&f, &host);
    }
    let dispatch = runtime.report(0).unwrap().dispatch;
    // Classes 1 and 2 are empty: class 3 receives eight turns, class 0 one.
    assert_eq!(dispatch.classes[3].selections, 16);
    assert_eq!(dispatch.classes[0].selections, 2);
    assert_eq!(dispatch.classes[0].last_sequence, 2);
    assert_eq!(dispatch.classes[3].last_sequence, 1);
    host.shutdown().unwrap();
}

#[test]
fn default_priority_round_robin_survives_a_host_and_runtime_restart() {
    let f = gated();
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    runtime.tick(&mut host).unwrap();
    assert_eq!(
        runtime.report(0).unwrap().dispatch.classes[0].last_sequence,
        1
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    runtime.tick(&mut host).unwrap();
    let class = &runtime.report(0).unwrap().dispatch.classes[0];
    assert_eq!(class.last_sequence, 2);
    assert_eq!(class.selections, 2);
    host.shutdown().unwrap();
}

#[test]
fn generated_steps_use_operator_template_priorities_and_migrate_without_new_artifacts() {
    let f = plan_support::fixture("gate", "gate");
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["priority"] = json!(3);
    config["routes"][0]["plan"]["templates"][1]["route"]["priority"] = json!(1);
    config["max_jobs_per_tick"] = json!(1);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    plan_support::until(&mut host, &mut runtime, |r| {
        !r.plans_after(0, 1).unwrap().is_empty()
    });
    let receipt = runtime.plans_after(0, 1).unwrap().remove(0);
    let core_count = host.report().unwrap().stored_artifacts;
    drop(runtime);
    let connection = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    connection.execute_batch("DROP TABLE runtime_job_priorities; DROP TABLE runtime_dispatch_state; DROP TABLE runtime_dispatch_classes;").unwrap();
    let runtime = reopen(&f, &host);
    for step in receipt.steps {
        let actual: u8 = connection
            .query_row(
                "SELECT priority FROM runtime_job_priorities WHERE sequence=?1",
                [step.sequence as i64],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            actual,
            match step.template_id.as_str() {
                "a" => 3,
                "b" => 1,
                _ => 0,
            }
        );
    }
    assert_eq!(host.report().unwrap().stored_artifacts, core_count);
    assert!(
        runtime
            .report(0)
            .unwrap()
            .dispatch
            .classes
            .iter()
            .all(|c| c.selections == 0)
    );
    host.shutdown().unwrap();
}

#[test]
fn invalid_priorities_are_rejected_and_default_zero_keeps_existing_identity() {
    let f = gated();
    let (mut host, runtime) = f.open();
    drop(runtime);
    let count = host.report().unwrap().stored_artifacts;
    let mut config = f.read("runtime.json");
    config["routes"][0]["priority"] = json!(0);
    f.write("runtime.json", config.clone());
    drop(reopen(&f, &host));
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    config["routes"][0]["priority"] = json!(4);
    f.write("runtime.json", config.clone());
    assert!(
        TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
        )
        .is_err()
    );
    config["routes"][0]["priority"] = json!(3);
    f.write("runtime.json", config);
    // An established route cannot gain priority through a changed configuration.
    assert!(
        TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
        )
        .is_err()
    );
    host.shutdown().unwrap();
}

#[test]
fn model_proposals_cannot_assign_their_own_step_priority() {
    let f = plan_support::fixture("normal", "normal");
    let mut model = f.read("planner.json");
    model["proposal"]["steps"][0]["priority"] = json!(3);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    plan_support::until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().failed_jobs == 1
    });
    // The closed candidate schema rejects this even before plan compilation.
    assert!(runtime.plans_after(0, 1).unwrap().is_empty());
    assert!(plan_support::artifacts(&host, "arcs.plan.v1").is_empty());
    assert!(!f.started("a") && !f.started("b"));
    plan_support::assert_no_authority(&host);
    host.shutdown().unwrap();
}

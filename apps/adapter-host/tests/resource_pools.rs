#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs::reasoning::{ContextSelection, ReasoningRequestV2};
use arcs_host::{
    Host, HostError,
    task_runtime::{ReasoningRoute, RuntimeConfig, TaskRuntime},
};
use parallel_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn configure(f: &Fixture, pools: Value, claims: &[(&str, Value)]) {
    let mut host = f.read("host.json");
    host["resource_pools"] = pools;
    for (adapter, claim) in claims {
        let config = host["adapters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|a| a["grant_file"] == format!("{adapter}-grant.json"))
            .unwrap();
        config["reasoning_resources"] = claim.clone();
    }
    f.write("host.json", host);
}
fn fixture(modes: &[(&str, &str)]) -> Fixture {
    let f = Fixture::new();
    for (id, mode) in modes {
        f.peer(id, mode, false);
        f.route(id, id);
    }
    let mut runtime = f.read("runtime.json");
    runtime["max_concurrent_reasoning"] = json!(4);
    f.write("runtime.json", runtime);
    f
}
fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick(host).unwrap();
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
fn used(host: &Host, name: &str) -> u64 {
    host.resource_usage()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .used
}
fn request(f: &Fixture, host: &Host, adapter: &str) -> ReasoningRequestV2 {
    let route: ReasoningRoute = serde_json::from_value(
        f.read("runtime.json")["routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["reasoner"]["adapter_id"] == adapter)
            .unwrap()
            .clone(),
    )
    .unwrap();
    let input = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.artifact.schema_id == route.input_schema_id)
        .unwrap()
        .artifact;
    ReasoningRequestV2 {
        request_id: format!("direct-{adapter}"),
        capability: route.reasoner,
        method: route.method,
        objective: route.objective,
        context: vec![ContextSelection {
            version_id: input.version_id,
            payload_fields: route.payload_fields,
        }],
        target_schema_id: route.target_schema_id,
        allowed_capabilities: route.allowed_capabilities,
        constraints: route.constraints,
        budget: route.budget,
    }
}

#[test]
fn two_adapters_share_one_gpu_while_an_unclaimed_operation_can_progress() {
    let f = fixture(&[("a", "gate"), ("b", "normal"), ("c", "normal")]);
    configure(
        &f,
        json!({"gpu":1}),
        &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
    );
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("a") && r.report(0).unwrap().ready_for_review == 1
    });
    assert!(f.started("c"));
    assert!(!f.started("b"));
    assert_eq!(used(&host, "gpu"), 1);
    let tick = runtime.tick(&mut host).unwrap();
    assert!(tick.failed.is_empty());
    assert!(
        tick.waiting_resources
            .iter()
            .any(|w| w.route_id == "b" && w.resource == "gpu" && w.available == 0)
    );
    let reports = host.report().unwrap().resource_pools;
    assert_eq!(reports[0].capacity, 1);
    assert_eq!(reports[0].available, 0);
    assert!(matches!(
        host.reason(request(&f, &host, "b")),
        Err(HostError::ResourceBusy { available: 0, .. })
    ));
    assert_eq!(f.count("b", "attempts"), 0);
    f.release("a");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    assert_eq!(used(&host, "gpu"), 0);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn weighted_claims_are_admitted_together_without_holding_partial_capacity() {
    let f = fixture(&[("a", "gate"), ("b", "gate"), ("c", "gate"), ("d", "normal")]);
    configure(
        &f,
        json!({"gpu":2,"ram_mb":8}),
        &[
            ("a", json!({"gpu":2})),
            ("b", json!({"gpu":1,"ram_mb":8})),
            ("c", json!({"ram_mb":8})),
            ("d", json!({"gpu":2})),
        ],
    );
    let mut config = f.read("runtime.json");
    config["routes"][3]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("c")
    });
    assert!(!f.started("b"));
    assert_eq!(used(&host, "gpu"), 2);
    assert_eq!(used(&host, "ram_mb"), 8);
    f.release("a");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.started("d") || runtime.report(0).unwrap().ready_for_review < 2 {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(!f.started("b"));
    assert_eq!(used(&host, "gpu"), 0);
    assert_eq!(used(&host, "ram_mb"), 8);
    f.release("c");
    until(&mut host, &mut runtime, |_| f.started("b"));
    assert_eq!(used(&host, "gpu"), 1);
    assert_eq!(used(&host, "ram_mb"), 8);
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert!(host.resource_usage().iter().all(|p| p.used == 0));
    host.shutdown().unwrap();
}

#[test]
fn invalid_pools_and_impossible_individual_claims_fail_before_storage_or_processes() {
    for case in [
        "zero_capacity",
        "negative_capacity",
        "oversized_capacity",
        "invalid_name",
        "unknown_pool",
        "zero_claim",
        "oversized_claim",
        "fractional",
        "too_many",
    ] {
        let f = fixture(&[("a", "normal")]);
        let mut host = f.read("host.json");
        host["resource_pools"] = json!({"gpu":1});
        host["adapters"][1]["reasoning_resources"] = json!({"gpu":1});
        match case {
            "zero_capacity" => host["resource_pools"]["gpu"] = json!(0),
            "negative_capacity" => host["resource_pools"]["gpu"] = json!(-1),
            "oversized_capacity" => host["resource_pools"]["gpu"] = json!(1_000_000_001u64),
            "invalid_name" => host["resource_pools"] = json!({"gpu / 0":1}),
            "unknown_pool" => host["adapters"][1]["reasoning_resources"] = json!({"missing":1}),
            "zero_claim" => host["adapters"][1]["reasoning_resources"]["gpu"] = json!(0),
            "oversized_claim" => host["adapters"][1]["reasoning_resources"]["gpu"] = json!(2),
            "fractional" => host["resource_pools"]["gpu"] = json!(1.5),
            "too_many" => {
                host["resource_pools"] = Value::Object(
                    (0..65)
                        .map(|n| (format!("resource{n}"), json!(1)))
                        .collect(),
                )
            }
            _ => unreachable!(),
        }
        f.write("host.json", host);
        assert!(Host::open(f.directory.join("host.json")).is_err(), "{case}");
        assert!(!f.directory.join("core.sqlite").exists());
        assert!(!f.directory.join("a.sqlite").exists());
    }
}

#[test]
fn cancelling_a_plan_keeps_its_capacity_until_the_cancelled_workers_reply() {
    let f = plan_support::fixture("gate", "gate");
    configure(
        &f,
        json!({"gpu":2}),
        &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
    );
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    assert_eq!(used(&host, "gpu"), 2);
    runtime
        .cancel_plan(&mut host, 1, "stop resource consumers".into())
        .unwrap();
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert_eq!(used(&host, "gpu"), 2);
    f.release("a");
    let deadline = Instant::now() + Duration::from_secs(20);
    while used(&host, "gpu") != 1 {
        runtime.tick(&mut host).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    f.release("b");
    while used(&host, "gpu") != 0 {
        runtime.tick(&mut host).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(plan_support::artifacts(&host, "arcs.solution_candidate.nearest.v1").is_empty());
    assert!(!f.started("join"));
    host.shutdown().unwrap();
}

#[test]
fn failures_invalid_results_and_timeouts_release_capacity_for_other_adapters() {
    for mode in ["fail", "invalid", "gate"] {
        let f = fixture(&[("a", mode), ("b", "normal")]);
        configure(
            &f,
            json!({"gpu":1}),
            &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
        );
        if mode == "gate" {
            let mut config = f.read("runtime.json");
            config["routes"][0]["budget"]["max_runtime_ms"] = json!(2000);
            f.write("runtime.json", config);
        }
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            r.report(0).unwrap().ready_for_review == 1
        });
        assert_eq!(runtime.report(0).unwrap().failed_jobs, 1, "{mode}");
        assert_eq!(used(&host, "gpu"), 0);
        assert_eq!(f.count("b", "attempts"), 1);
        host.shutdown().unwrap();
    }
}

#[test]
fn restart_replays_the_same_invocation_and_rebuilds_logical_capacity_from_new_live_work() {
    let f = fixture(&[("a", "gate"), ("b", "normal")]);
    configure(
        &f,
        json!({"gpu":1}),
        &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
    );
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| f.started("a"));
    assert_eq!(used(&host, "gpu"), 1);
    assert!(!f.started("b"));
    host.shutdown().unwrap();
    assert_eq!(used(&host, "gpu"), 0);
    drop(runtime);
    drop(host);
    f.release("a");
    let (mut host, mut runtime) = f.open();
    assert_eq!(used(&host, "gpu"), 0);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(f.count("a", "results"), 1);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(used(&host, "gpu"), 0);
    host.shutdown().unwrap();
}

#[test]
fn separate_runtimes_in_one_host_share_capacity_and_synchronous_waiting_is_not_a_failure() {
    let f = fixture(&[("a", "gate"), ("b", "normal")]);
    configure(
        &f,
        json!({"gpu":1}),
        &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
    );
    let mut first = f.read("runtime.json");
    let mut second = first.clone();
    first["routes"] = json!([first["routes"][0]]);
    second["routes"] = json!([second["routes"][1]]);
    second["runtime_id"] = json!("second-runtime");
    second["journal"] = json!("second.sqlite");
    second["max_concurrent_reasoning"] = json!(1);
    f.write("runtime.json", first);
    f.write("second-runtime.json", second);
    let (mut host, mut one) = f.open();
    until(&mut host, &mut one, |_| f.started("a"));
    let mut two = TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("second-runtime.json")).unwrap(),
    )
    .unwrap();
    let report = two.tick(&mut host).unwrap();
    assert!(report.failed.is_empty());
    assert_eq!(report.waiting_resources.len(), 1);
    assert_eq!(two.report(0).unwrap().failed_jobs, 0);
    assert!(!f.started("b"));
    f.release("a");
    until(&mut host, &mut one, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    until(&mut host, &mut two, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(used(&host, "gpu"), 0);
    host.shutdown().unwrap();
}

#[test]
fn cached_results_need_no_capacity_and_still_require_the_same_request_fingerprint() {
    let f = fixture(&[("a", "gate"), ("b", "normal"), ("c", "gate")]);
    configure(
        &f,
        json!({"gpu":1}),
        &[("a", json!({"gpu":1})), ("b", json!({"gpu":1}))],
    );
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(2);
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    config["routes"][2]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    let direct = request(&f, &host, "b");
    let original = host.reason(direct.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f.started("a") || !f.started("c") {
        runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(used(&host, "gpu"), 1);
    let replay = host.reason(direct.clone()).unwrap();
    assert_eq!(replay.result_version_id, original.result_version_id);
    let mut changed = direct;
    changed.objective = "changed request identity".into();
    assert!(matches!(host.reason(changed), Err(HostError::Reasoning(_))));
    assert_eq!(f.count("b", "attempts"), 2);
    // Reproduce a lost task completion while both worker slots and the GPU
    // are occupied. A validated cached result must not start a third worker.
    rusqlite::Connection::open(f.directory.join("runtime.sqlite"))
        .unwrap()
        .execute(
            "UPDATE runtime_jobs SET state='running',result_json=NULL WHERE route_id='b'",
            [],
        )
        .unwrap();
    runtime.tick_at(&mut host, i64::MAX as u64).unwrap();
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 1);
    assert_eq!(f.count("b", "attempts"), 2);
    assert_eq!(used(&host, "gpu"), 1);
    f.release("a");
    f.release("c");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    host.shutdown().unwrap();
}

#[test]
fn combined_participant_claims_that_exceed_capacity_are_rejected_before_any_data_or_reasoning_dispatch()
 {
    let f = fixture(&[("a", "normal"), ("b", "normal")]);
    configure(
        &f,
        json!({"ram_mb":8}),
        &[("a", json!({"ram_mb":5})), ("b", json!({"ram_mb":5}))],
    );
    let mut config = f.read("runtime.json");
    for route in config["routes"].as_array_mut().unwrap() {
        route["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    }
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    runtime.tick(&mut host).unwrap();
    let request = request(&f, &host, "a");
    let before = host.report().unwrap().stored_artifacts;
    let input = arcs::adapters::input_data::InputDataSelection {
        name: "source".into(),
        source: arcs::adapters::CapabilityRef::new("b", "solution.propose"),
        source_version_id: request.context[0].version_id.clone(),
        reference_pointer: "/data".into(),
    };
    // Each individual claim fits. Their combined requirement must fail before
    // independent data-contract checks and before any Core dispatch is recorded.
    assert!(
        matches!(host.reason_with_data(request,&[input]),Err(HostError::InvalidConfig(message)) if message.contains("combined reasoning operation"))
    );
    assert_eq!(host.report().unwrap().stored_artifacts, before);
    assert_eq!(used(&host, "ram_mb"), 0);
    assert_eq!(f.count("a", "attempts"), 0);
    assert_eq!(f.count("b", "attempts"), 0);
    host.shutdown().unwrap();
}

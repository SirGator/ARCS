//! Gemeinsame Planerprozesse für Ausführungs-, Abbruch- und Recovery-Tests.
#![allow(dead_code)]
use crate::parallel_support::Fixture;
use arcs_host::{Host, task_runtime::TaskRuntime};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const PLANNER: &str = include_str!("planner_peer.py");
pub fn proposal() -> Value {
    json!({"steps":[{"id":"left","template":"a"},{"id":"right","template":"b"},{"id":"total","template":"join","dependencies":[{"step_id":"left","payload_fields":["value"]},{"step_id":"right","payload_fields":["value"]}]}]})
}
pub fn fixture(a: &str, b: &str) -> Fixture {
    let f = Fixture::new();
    for (id, mode) in [("a", a), ("b", b), ("join", "join")] {
        f.peer(id, mode, false);
        f.route(id, id);
    }
    let mut config = f.read("runtime.json");
    let templates: Vec<_> = config["routes"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, route)| {
            let mut route = route.clone();
            if i == 2 {
                route["budget"]["max_context_items"] = json!(4);
            }
            json!({"route":route,"export_fields":["value"]})
        })
        .collect();
    let mut root = config["routes"][0].clone();
    root["id"] = json!("plan-task");
    root["reasoner"]["adapter_id"] = json!("planner");
    root["target_schema_id"] = json!("arcs.plan_candidate.v1");
    root["budget"]["max_context_bytes"] = json!(16384);
    root["plan"] = json!({"max_steps":3,"max_total_runtime_ms":30000,"templates":templates});
    config["routes"] = json!([root]);
    config["max_pending_jobs"] = json!(4);
    f.write("runtime.json", config);
    f.write("planner.json", json!({"proposal":proposal()}));
    std::fs::write(f.directory.join("planner.py"), PLANNER).unwrap();
    let mut grant = f.read("a-grant.json");
    grant["adapter_id"] = json!("planner");
    f.write("planner-grant.json", grant);
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"planner.py"},{"path":"planner.json"}],"grant_file":"planner-grant.json","timeout_ms":15000}));
    f.write("host.json", host);
    f
}
pub fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let until = Instant::now() + Duration::from_secs(25);
    loop {
        let report = runtime.tick(host).unwrap();
        if !report.failed.is_empty() {
            eprintln!("plan fixture task failures: {:?}", report.failed);
        }
        if condition(runtime) {
            return;
        }
        assert!(Instant::now() < until, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}
pub fn artifacts(host: &Host, schema: &str) -> Vec<arcs::core::Artifact> {
    host.store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|e| e.artifact)
        .filter(|a| a.schema_id.0 == schema)
        .collect()
}
pub fn assert_no_authority(host: &Host) {
    assert!(host.store().committed_after(0).unwrap().iter().all(|e| {
        !["action", "execution", "approval"].contains(&e.artifact.artifact_type.as_str())
    }));
}

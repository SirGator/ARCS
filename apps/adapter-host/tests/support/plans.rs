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

pub const CONVERTED: &str = "arcs.measurement.converted_test.v1";

pub fn add_dependency_transform(f: &Fixture, config: &mut Value) {
    let projected = "arcs.projection.plan_test.v1";
    f.write(
        "projection.schema.json",
        json!({
            "$id":projected,"type":"object","required":["value"],"additionalProperties":false,
            "properties":{"value":{"type":"number"}}
        }),
    );
    f.write("converted.schema.json", json!({
        "$id":CONVERTED,"type":"object","required":["value","label"],"additionalProperties":false,
        "properties":{"value":{"type":"number"},"label":{"type":"string"}}
    }));
    let mut host = f.read("host.json");
    host["schemas"].as_array_mut().unwrap().extend([
        json!("projection.schema.json"),
        json!("converted.schema.json"),
    ]);
    host["adapters"].as_array_mut().unwrap().push(json!({
        "program":"/usr/bin/python3","args":[{"path":"convert.py"}],
        "grant_file":"convert-grant.json","timeout_ms":15000
    }));
    f.write("host.json", host);
    f.write(
        "convert-grant.json",
        json!({
            "adapter_id":"converter","producer_class":"adapter","enabled_capabilities":["convert"],
            "granted_permissions":["data.transform"],"assigned_trust":"high",
            "max_payload_bytes":4096,"max_external_reference_bytes":256
        }),
    );
    std::fs::write(f.directory.join("convert.py"), r#"import json, pathlib, sys, time
root = pathlib.Path(__file__).parent
for line in sys.stdin:
    packet = json.loads(line)
    msg = packet['message']
    kind = msg['type']
    if kind == 'hello':
        reply = {'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'converter','adapter_version':'1','capabilities':[{'id':'convert','contract':{'kind':'transform','accepts':['arcs.projection.plan_test.v1'],'emits':['arcs.measurement.converted_test.v1']},'required_permissions':['data.transform']}]}}
    elif kind == 'transform':
        inv = msg['invocation']
        assert inv['input_schema_id'] == 'arcs.projection.plan_test.v1'
        assert set(inv['input_payload']) == {'value'}
        with (root / 'convert.started').open('a') as calls:
            calls.write(inv['invocation_id'] + '\n')
        while not (root / 'convert.release').exists():
            time.sleep(.003)
        reply = {'type':'transform_result','response':{'invocation_id':inv['invocation_id'],'external_reference':'converted','payload':{'value':inv['input_payload']['value']+10,'label':'not exported'}}}
    elif kind == 'shutdown':
        reply = {'type':'bye'}
    else:
        raise RuntimeError(kind)
    print(json.dumps({'protocol_version':1,'request_id':packet['request_id'],'message':reply}),flush=True)
    if kind == 'shutdown':
        break
"#).unwrap();
    let mut join = f.read("join.json");
    join["parent_schema_id"] = json!(CONVERTED);
    f.write("join.json", join);
    let join = &mut config["routes"][0]["plan"]["templates"][2];
    join["accepted_dependency_schemas"] = json!([CONVERTED]);
    join["dependency_transforms"] = json!([{
        "source_schema_id":"arcs.solution_candidate.nearest.v1",
        "projection":{"schema_id":projected,"payload_fields":["value"]},
        "capability":{"adapter_id":"converter","capability_id":"convert"},
        "output_schema_id":CONVERTED,
        "output_fields":["value"]
    }]);
}

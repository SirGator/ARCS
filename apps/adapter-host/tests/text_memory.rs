#[path = "support/memory.rs"]
mod memory;
#[path = "support/parallel.rs"]
mod parallel;
mod support;

use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use parallel::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn fixture() -> Fixture {
    let f = Fixture::new();
    f.peer("main", "normal", false);
    f.peer("independent", "normal", false);
    f.route("memory", "main");
    f.route("other", "independent");
    let mut config = f.read("runtime.json");
    let route = &mut config["routes"][0];
    route["memory"] = memory::policy("project:p");
    route["memory"]["scopes"][0]["text_query"] = json!({"text":"calibration"});
    route["memory"]["search"]["max_visited"] = json!(3);
    route["budget"]["max_context_items"] = json!(3);
    f.write("runtime.json", config);
    f
}
fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(runtime) {
            return;
        }
        assert!(Instant::now() < end, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn input(f: &Fixture, value: Value) {
    let mut profile = f.read("input-profile.json");
    if value.is_null() {
        profile["events"][0]["payload"]
            .as_object_mut()
            .unwrap()
            .remove("private_note");
    } else {
        profile["events"][0]["payload"]["private_note"] = value;
    }
    f.write("input-profile.json", profile);
}

#[test]
fn rebuilding_old_memory_does_not_freeze_partial_context_or_hold_a_worker() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    let wanted = memory::append(&host, "old-calibration", "project:p");
    for n in 0..70 {
        memory::append(&host, &format!("noise-{n}"), "project:p");
    }
    memory::append(&host, "private-calibration", "project:other");
    host.poll_inputs().unwrap();
    let tick = runtime.tick(&mut host).unwrap();
    assert!(tick.failed.is_empty());
    assert!(
        tick.waiting_resources
            .iter()
            .any(|wait| wait.resource == "memory-text-index")
    );
    assert!(memory::snapshots(&host).is_empty());
    assert_eq!(f.count("main", "attempts"), 0);
    until(&mut host, &mut runtime, |runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(f.count("independent", "attempts"), 1);
    assert_eq!(f.count("main", "attempts"), 1);
    let record = memory::snapshots(&host).remove(0);
    assert_eq!(record.payload["selected"].as_array().unwrap().len(), 1);
    assert_eq!(
        record.payload["selected"][0]["selection"]["version_id"],
        wanted.version_id.0
    );
    assert_eq!(
        record.payload["trace"]["text_indexes"][0]["skipped_oversized"],
        0
    );
    let done = runtime
        .completed_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|done| done.route_id == "memory")
        .unwrap();
    let request = host.store().get(&done.request_version_id).unwrap().unwrap();
    assert!(
        request.payload["invocation"]["context"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["version_id"] == wanted.version_id.0)
    );
    host.shutdown().unwrap();
}

#[test]
fn lost_completion_replays_saved_text_context_without_rebuilding_a_deleted_index() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    memory::append(&host, "first-calibration", "project:p");
    until(&mut host, &mut runtime, |runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    let original = runtime.completed_after(0, 10).unwrap();
    let record = memory::snapshots(&host).remove(0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute(
        "UPDATE runtime_jobs SET state='running',result_json=NULL",
        [],
    )
    .unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    host.store().clear_text_indexes().unwrap();
    memory::append(&host, "later-calibration", "project:p");
    let count = host.store().len().unwrap();
    until(&mut host, &mut runtime, |runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(runtime.completed_after(0, 10).unwrap(), original);
    assert_eq!(memory::snapshots(&host)[0].version_id, record.version_id);
    assert_eq!(host.store().len().unwrap(), count);
    assert_eq!(f.count("main", "attempts"), 1);
    let db = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='artifact_text_fts_v1')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !exists,
        "cached memory unexpectedly required a fresh text query"
    );
    host.shutdown().unwrap();
}

#[test]
fn private_input_query_selects_memory_without_exporting_the_query_field() {
    let f = fixture();
    input(&f, json!("calibration"));
    let mut config = f.read("runtime.json");
    config["routes"][0]["memory"]["scopes"][0]["text_query"] =
        json!({"input_fields":["private_note"]});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let wanted = memory::append(&host, "calibration", "project:p");
    until(&mut host, &mut runtime, |runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    let done = runtime
        .completed_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|done| done.route_id == "memory")
        .unwrap();
    let request = host.store().get(&done.request_version_id).unwrap().unwrap();
    let context = request.payload["invocation"]["context"].as_array().unwrap();
    assert_eq!(context.len(), 2);
    assert!(
        context
            .iter()
            .any(|item| item["version_id"] == wanted.version_id.0)
    );
    assert!(
        context
            .iter()
            .all(|item| item["payload"].get("private_note").is_none())
    );
    assert!(
        !request.payload["invocation"]["constraints"]
            .to_string()
            .contains("calibration")
    );
    host.shutdown().unwrap();
}

#[test]
fn missing_empty_and_unmatched_queries_are_isolated_without_a_default_recall() {
    for value in [Value::Null, json!(""), json!("***"), json!("no-match")] {
        let f = fixture();
        input(&f, value.clone());
        let mut config = f.read("runtime.json");
        config["routes"][0]["memory"]["scopes"][0]["text_query"] =
            json!({"input_fields":["private_note"]});
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        memory::append(&host, "calibration", "project:p");
        if value == json!("no-match") {
            until(&mut host, &mut runtime, |runtime| {
                runtime.report(0).unwrap().ready_for_review == 2
            });
            assert!(
                memory::snapshots(&host)[0].payload["selected"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            let end = Instant::now() + Duration::from_secs(30);
            while runtime.report(0).unwrap().ready_for_review < 1 {
                runtime.tick(&mut host).unwrap();
                assert!(Instant::now() < end);
            }
            assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
            assert_eq!(f.count("main", "attempts"), 0);
            assert!(memory::snapshots(&host).is_empty());
        }
        assert_eq!(f.count("independent", "attempts"), 1);
        host.shutdown().unwrap();
    }
}

#[test]
fn invalid_text_policies_do_not_open_an_operational_journal() {
    for query in [
        json!({}),
        json!({"text":"***"}),
        json!({"text":"a ".repeat(33)}),
        json!({"text":"word","input_fields":["same","same"]}),
    ] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        config["routes"][0]["memory"]["scopes"][0]["text_query"] = query;
        f.write("runtime.json", config);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
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
}

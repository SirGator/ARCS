#[path = "support/parallel.rs"]
mod parallel_support;
mod support;
use arcs::adapters::reconnect::{ConnectionFailure, ConnectionState};
use arcs_host::{Host, task_runtime::TaskRuntime};
use parallel_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn fixture(mode: &str, attempts: u8) -> Fixture {
    let f = Fixture::new();
    f.peer("a", mode, false);
    f.peer("b", "normal", true);
    f.route("main", "a");
    f.route("independent", "b");
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    let prefix = "boot_path=root/(c['id']+'.boots')\nboot=int(boot_path.read_text())+1 if boot_path.exists() else 1\nboot_path.write_text(str(boot))\nif c.get('mode') in ('recover','slow_hello','changed','timeout_hello'):\n c['mode']='fail' if boot==1 else 'normal'\n";
    let source=source.replace("n=0\n",&(prefix.to_owned()+"n=0\n")).replace(" if kind=='hello':"," if kind=='hello':\n  if c['id']=='a' and boot>1 and c.get('hello_gate'):\n   (root/'reconnect.started').write_text(str(boot))\n   while not (root/'reconnect.release').exists(): time.sleep(.003)");
    let source = source.replace(
        "'adapter_version':'1'",
        "'adapter_version':('changed' if c.get('change_manifest') and boot>1 else '1')",
    );
    std::fs::write(path, source).unwrap();
    let mut profile = f.read("a.json");
    profile["hello_gate"] = json!(mode == "slow_hello" || mode == "timeout_hello");
    profile["change_manifest"] = json!(mode == "changed");
    f.write("a.json", profile);
    let mut host = f.read("host.json");
    host["adapters"][1]["reasoning_reconnect"] = json!({"max_attempts":attempts,"delay_ms":10,"handshake_timeout_ms":if mode=="timeout_hello"{100}else{1500}});
    f.write("host.json", host);
    let mut config = f.read("runtime.json");
    let route = &mut config["routes"][0];
    let choice =
        json!({"capability":route["reasoner"],"method":route["method"],"budget":route["budget"]});
    route["reasoning_recovery"] = json!({"alternatives":[choice],"delay_ms":0});
    f.write("runtime.json", config);
    f
}
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    condition: impl Fn(&Host, &TaskRuntime) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick(host).unwrap();
        if condition(host, runtime) {
            return;
        }
        assert!(
            Instant::now() < end,
            "{:?} {:?}",
            runtime.report(0).unwrap(),
            host.adapter_reconnections().unwrap()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn boots(f: &Fixture, id: &str) -> u32 {
    std::fs::read_to_string(f.directory.join(format!("{id}.boots")))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn a_repaired_session_finishes_an_explicit_retry_and_keeps_original_attempts() {
    let f = fixture("recover", 2);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, runtime| {
        runtime.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 1);
    let report = host.adapter_reconnections().unwrap();
    assert_eq!(report[0].connection.record.state, ConnectionState::Healthy);
    assert_eq!(report[0].connection.record.attempt, 1);
    assert_eq!(report[0].connection.record.generation, 1);
    let attempts = host
        .store()
        .committed_after(0)
        .unwrap()
        .iter()
        .filter(|event| event.artifact.schema_id.0 == "arcs.reasoning_attempt.v1")
        .count();
    assert_eq!(attempts, 3);
    host.shutdown().unwrap();
}
#[test]
fn a_stalled_handshake_owns_capacity_without_blocking_other_jobs_or_sensors() {
    let f = fixture("slow_hello", 2);
    let mut config = f.read("host.json");
    config["resource_pools"] = json!({"gpu":1});
    config["adapters"][1]["reasoning_resources"] = json!({"gpu":1});
    f.write("host.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, r| {
        f.directory.join("reconnect.started").exists() && r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(host.resource_usage()[0].used, 1);
    assert_eq!(
        host.adapter_reconnections().unwrap()[0]
            .connection
            .record
            .state,
        ConnectionState::Connecting
    );
    let before = host.store().len().unwrap();
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert!(host.store().len().unwrap() > before);
    assert_eq!(f.count("a", "attempts"), 1);
    std::fs::write(f.directory.join("reconnect.release"), "go").unwrap();
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(host.resource_usage()[0].used, 0);
    host.shutdown().unwrap();
}
#[test]
fn changed_metadata_never_replaces_the_registered_contract_or_calls_the_new_session() {
    let f = fixture("changed", 3);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |h, r| {
        h.adapter_reconnections().unwrap()[0]
            .connection
            .record
            .state
            == ConnectionState::Rejected
            && r.report(0).unwrap().failed_jobs == 1
    });
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(
        host.adapter_catalog()
            .into_iter()
            .find(|adapter| adapter.adapter_id.0 == "a")
            .unwrap()
            .adapter_version,
        "1"
    );
    assert_eq!(
        host.adapter_reconnections().unwrap()[0]
            .connection
            .record
            .failure,
        Some(ConnectionFailure::Contract)
    );
    host.shutdown().unwrap();
}
#[test]
fn timeouts_exhaust_the_episode_and_a_verified_new_host_does_not_resurrect_failed_tasks() {
    let f = fixture("timeout_hello", 2);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |h, r| {
        h.adapter_reconnections().unwrap()[0]
            .connection
            .record
            .state
            == ConnectionState::Exhausted
            && r.report(0).unwrap().failed_jobs == 1
    });
    assert_eq!(boots(&f, "a"), 3);
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(boots(&f, "a"), 3);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    std::fs::write(f.directory.join("reconnect.release"), "repaired").unwrap();
    let (mut host, mut runtime) = f.open();
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(
        host.adapter_reconnections().unwrap()[0]
            .connection
            .record
            .state,
        ConnectionState::Healthy
    );
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}
#[test]
fn shutdown_cancels_an_unanswered_handshake_and_records_the_consumed_attempt() {
    let f = fixture("slow_hello", 2);
    let mut config = f.read("host.json");
    config["adapters"][1]["reasoning_reconnect"]["handshake_timeout_ms"] = json!(3_600_000);
    f.write("host.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| {
        f.directory.join("reconnect.started").exists()
    });
    let started = Instant::now();
    host.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    let status = host.adapter_reconnections().unwrap();
    assert_eq!(status[0].connection.record.attempt, 1);
    assert_eq!(
        status[0].connection.record.failure,
        Some(ConnectionFailure::Interrupted)
    );
    assert_eq!(f.count("a", "attempts"), 1);
}
#[test]
fn automatic_reconnect_has_one_live_owner_per_node_and_core() {
    let f = fixture("recover", 2);
    let (mut host, runtime) = f.open();
    assert!(Host::open(f.directory.join("host.json")).is_err());
    assert_eq!(boots(&f, "a"), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    assert_eq!(boots(&f, "a"), 2);
    host.shutdown().unwrap();
}
#[test]
fn invalid_policies_and_multirole_processes_are_rejected() {
    for mode in ["attempts", "delay", "timeout", "multirole"] {
        let f = fixture("recover", 2);
        let mut config = f.read("host.json");
        match mode {
            "attempts" => config["adapters"][1]["reasoning_reconnect"]["max_attempts"] = json!(9),
            "delay" => config["adapters"][1]["reasoning_reconnect"]["delay_ms"] = json!(0),
            "timeout" => {
                config["adapters"][1]["reasoning_reconnect"]["handshake_timeout_ms"] =
                    json!(3_600_001)
            }
            _ => {
                config["adapters"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("reasoning_reconnect");
                config["adapters"][2]["reasoning_reconnect"] =
                    json!({"max_attempts":1,"delay_ms":1,"handshake_timeout_ms":100});
            }
        }
        f.write("host.json", config);
        assert!(Host::open(f.directory.join("host.json")).is_err());
        assert!(!f.directory.join("runtime.sqlite").exists());
    }
}

#[test]
fn a_core_publication_failure_retains_the_handshake_and_never_installs_an_uncommitted_session() {
    let f = fixture("recover", 2);
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_session BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.adapter_reconnect_state.v1' AND json_extract(NEW.artifact_json,'$.payload.state')='healthy' AND json_extract(NEW.artifact_json,'$.payload.attempt')>0 BEGIN SELECT RAISE(ABORT,'interrupted session commit'); END;").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("a", "attempts"), 1);
    let status = host.adapter_reconnections().unwrap();
    assert_eq!(
        status[0].connection.record.state,
        ConnectionState::Connecting
    );
    assert!(status[0].quarantined);
    assert!(status[0].handshake_in_progress);
    db.execute_batch("DROP TRIGGER fail_session").unwrap();
    drop(db);
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
}

#[test]
fn queued_new_work_waits_for_reconnection_without_retrying_a_failed_task() {
    let f = fixture("recover", 2);
    let mut config = f.read("runtime.json");
    config["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("reasoning_recovery");
    f.write("runtime.json", config);
    let mut profile = f.read("input-profile.json");
    let mut second = profile["events"][0].clone();
    second["id"] = json!("second");
    second["subject"] = json!("second");
    second["reference"] = json!("task:second");
    profile["events"].as_array_mut().unwrap().push(second);
    f.write("input-profile.json", profile);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, r| {
        let report = r.report(0).unwrap();
        report.failed_jobs == 1 && report.ready_for_review == 3
    });
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("b", "attempts"), 2);
    host.shutdown().unwrap();
}

#[test]
fn handshake_timeout_does_not_shorten_the_reconnected_reasoning_deadline() {
    let f = fixture("recover", 2);
    let mut config = f.read("host.json");
    config["adapters"][1]["reasoning_reconnect"]["handshake_timeout_ms"] = json!(100);
    f.write("host.json", config);
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        source.replace(
            "  inv=m['invocation'];",
            "  if c['id']=='a' and boot>1: time.sleep(.25)\n  inv=m['invocation'];",
        ),
    )
    .unwrap();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(f.count("a", "attempts"), 2);
    host.shutdown().unwrap();
}

#[test]
fn fresh_processes_inherit_only_the_operator_allowlist_and_reports_have_no_environment_values() {
    let f = fixture("recover", 2);
    let mut config = f.read("host.json");
    config["adapters"][1]["inherit_env"] = json!(["ARCS_RECONNECT_TEST_TOKEN"]);
    f.write("host.json", config);
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path,source.replace("boot_path=root/", "import os\nif c['id']=='a':\n with open(root/'environment.jsonl','a') as log: log.write(json.dumps({key:os.environ.get(key) for key in ['ARCS_RECONNECT_TEST_TOKEN','ARCS_RECONNECT_OTHER_SECRET']})+'\\n')\nboot_path=root/")).unwrap();
    let mut runtime = f.read("runtime.json");
    runtime["interval_ms"] = json!(20);
    f.write("runtime.json", runtime);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .args([
            "--config",
            f.directory.join("host.json").to_str().unwrap(),
            "--runtime",
            f.directory.join("runtime.json").to_str().unwrap(),
            "--ticks",
            "30",
        ])
        .env(
            "ARCS_RECONNECT_TEST_TOKEN",
            "synthetic-allowed-reconnect-token",
        )
        .env("ARCS_RECONNECT_OTHER_SECRET", "synthetic-not-forwarded")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["ready_for_review"], 2);
    assert!(!text.contains("synthetic-allowed-reconnect-token"));
    assert!(!text.contains("synthetic-not-forwarded"));
    let logs = std::fs::read_to_string(f.directory.join("environment.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(logs.len(), 2);
    for entry in logs {
        assert_eq!(
            entry["ARCS_RECONNECT_TEST_TOKEN"],
            "synthetic-allowed-reconnect-token"
        );
        assert!(entry["ARCS_RECONNECT_OTHER_SECRET"].is_null());
    }
    let core = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
    let exposed:i64=core.query_row("SELECT COUNT(*) FROM artifact_versions WHERE artifact_json LIKE '%synthetic-allowed-reconnect-token%' OR artifact_json LIKE '%synthetic-not-forwarded%'",[],|row|row.get(0)).unwrap();
    assert_eq!(exposed, 0);
}

#[test]
fn occupied_shared_resources_do_not_consume_a_reconnect_attempt() {
    let f = fixture("recover", 2);
    f.peer("holder", "gate", false);
    f.route("holder", "holder");
    let mut host_config = f.read("host.json");
    host_config["resource_pools"] = json!({"gpu":1});
    for index in [1, 3] {
        host_config["adapters"][index]["reasoning_resources"] = json!({"gpu":1});
    }
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["routes"][2]["activation"] = json!({"kind":"at_time","unix_ms":1001});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        runtime.tick_at(&mut host, 1000).unwrap();
        if host
            .store()
            .committed_after(0)
            .unwrap()
            .iter()
            .any(|event| {
                event.artifact.schema_id.0 == "arcs.reasoning_attempt.v1"
                    && event.artifact.payload["outcome"]["kind"] == "failed"
            })
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    loop {
        runtime.tick_at(&mut host, 1001).unwrap();
        if f.started("holder") {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let due = host.adapter_reconnections().unwrap()[0]
        .connection
        .record
        .not_before_unix_ms
        .unwrap();
    for _ in 0..3 {
        runtime.tick_at(&mut host, due).unwrap();
    }
    let status = host.adapter_reconnections().unwrap();
    assert_eq!(status[0].connection.record.state, ConnectionState::Waiting);
    assert_eq!(status[0].connection.record.attempt, 0);
    assert_eq!(boots(&f, "a"), 1);
    assert_eq!(host.resource_usage()[0].used, 1);
    f.release("holder");
    loop {
        runtime.tick_at(&mut host, due).unwrap();
        if runtime.report(0).unwrap().ready_for_review == 3 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(boots(&f, "a"), 2);
    assert_eq!(host.resource_usage()[0].used, 0);
    host.shutdown().unwrap();
}

#[test]
fn a_stopped_host_does_not_restart_and_dropping_releases_node_ownership_after_cleanup() {
    let f = fixture("slow_hello", 2);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, _| {
        f.directory.join("reconnect.started").exists()
    });
    host.shutdown().unwrap();
    for _ in 0..3 {
        host.poll_reconnections().unwrap();
    }
    assert_eq!(boots(&f, "a"), 2);
    drop(runtime);
    drop(host);
    std::fs::write(f.directory.join("reconnect.release"), "repaired").unwrap();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(boots(&f, "a"), 3);
    host.shutdown().unwrap();
}

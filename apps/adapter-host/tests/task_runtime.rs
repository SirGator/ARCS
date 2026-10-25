mod support;
use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/roles");
        for name in [
            "input-profile.json",
            "input-grant.json",
            "solver-profile.json",
            "solver-grant.json",
            "model-profile.json",
            "model-grant.json",
        ] {
            std::fs::copy(source.join(name), directory.join(name)).unwrap();
        }
        let fixture = Self { directory };
        // Process startup can contend with workspace tests; reasoning has its
        // own shorter budget, unchanged by this connection timeout.
        fixture.write("host.json",json!({"config_version":1,"database":"db/core.sqlite","schemas":[root.join("adapters/example-reasoner/schemas/task.schema.json"),root.join("adapters/example-reasoner/schemas/candidate.schema.json")],"adapters":[
            {"program":support::adapter_binary("arcs-json-input"),"profile":"input-profile.json","args":[{"path":"db/input.sqlite"}],"grant_file":"input-grant.json","timeout_ms":15000},
            {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"solver-profile.json","args":[{"path":"db/solver.sqlite"}],"grant_file":"solver-grant.json","timeout_ms":15000},
            {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"model-profile.json","args":[{"path":"db/model.sqlite"}],"grant_file":"model-grant.json","timeout_ms":15000}]}));
        fixture.write("runtime.json",json!({"config_version":1,"runtime_id":"nearest-runtime","journal":"db/runtime.sqlite","interval_ms":5,"max_events_per_tick":1000,"max_jobs_per_tick":1,"max_pending_jobs":2,"routes":[{
            "id":"solve","source":{"adapter_id":"example.input","capability_id":"task.input"},"input_schema_id":"arcs.task.nearest.v1","payload_fields":["values","target"],
            "reasoner":{"adapter_id":"example.solver","capability_id":"solution.propose"},"method":"solver","objective":"Find the nearest listed value","target_schema_id":"arcs.solution_candidate.nearest.v1","allowed_capabilities":[],"constraints":{},
            "budget":{"max_context_items":1,"max_context_bytes":8192,"max_output_bytes":4096,"max_candidates":1,"max_runtime_ms":1000,"max_iterations":4096,"max_output_tokens":null}}]}));
        // Source capability is defined by the actual profile, not by the runtime.
        let profile = fixture.read("input-profile.json");
        let mut config = fixture.read("runtime.json");
        config["routes"][0]["source"]["capability_id"] = profile["capability_id"].clone();
        fixture.write("runtime.json", config);
        fixture
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn host(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn config(&self) -> RuntimeConfig {
        RuntimeConfig::load(self.directory.join("runtime.json")).unwrap()
    }
    fn runtime(&self, host: &Host) -> TaskRuntime {
        TaskRuntime::open(host, self.config()).unwrap()
    }
    fn db(&self, name: &str) -> rusqlite::Connection {
        let db =
            rusqlite::Connection::open(self.directory.join(format!("db/{name}.sqlite"))).unwrap();
        db.busy_timeout(Duration::from_secs(2)).unwrap();
        db
    }
    fn enqueue(&self, event: Value) -> std::process::Output {
        self.write("event.json", event);
        Command::new(support::adapter_binary("arcs-json-input"))
            .arg("--enqueue")
            .arg(self.directory.join("input-profile.json"))
            .arg(self.directory.join("db/input.sqlite"))
            .arg(self.directory.join("event.json"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn assert_no_authority(host: &Host) {
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|event| !["approval", "action", "execution"]
                .contains(&event.artifact.artifact_type.as_str()))
    );
    assert_eq!(host.report().unwrap().world_revision, 0);
}
fn scalar(db: &rusqlite::Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn acked_input_is_discovered_after_restart_and_candidates_are_handed_to_review() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let input = host.poll_inputs().unwrap().remove(0);
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let tick = runtime.tick(&mut host).unwrap();
    assert_eq!(tick.discovered_jobs, 1);
    let done = &tick.ready_for_review[0];
    assert_eq!(done.input_version_id, input.version_id);
    let candidate = host
        .store()
        .get(&done.candidate_version_ids[0])
        .unwrap()
        .unwrap();
    assert_eq!(candidate.payload["value"], 2.0);
    assert!(
        host.store()
            .get(&done.request_version_id)
            .unwrap()
            .unwrap()
            .payload["invocation"]["context"][0]["payload"]
            .get("private_note")
            .is_none()
    );
    assert_eq!(
        runtime.completed_after(0, 1).unwrap(),
        tick.ready_for_review
    );
    assert!(
        runtime
            .completed_after(done.sequence, 1)
            .unwrap()
            .is_empty()
    );
    assert_no_authority(&host);
    let all = host.store().committed_after(0).unwrap();
    let mut paged = Vec::new();
    let mut cursor = 0;
    loop {
        let page = host.store().committed_page_after(cursor, 2).unwrap();
        if page.is_empty() {
            break;
        }
        cursor = page.last().unwrap().sequence;
        paged.extend(page);
    }
    assert_eq!(paged, all);
    assert!(host.store().committed_page_after(0, 0).unwrap().is_empty());
    let count = host.report().unwrap().stored_artifacts;
    assert!(runtime.tick(&mut host).unwrap().ready_for_review.is_empty());
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    host.shutdown().unwrap();
}

#[test]
fn interrupted_completion_reuses_core_and_driver_journals() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = runtime.tick(&mut host).unwrap().ready_for_review.remove(0);
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    // Crash window: backend and Core succeeded, operational completion did not commit.
    fixture
        .db("runtime")
        .execute(
            "UPDATE runtime_jobs SET state='running',result_json=NULL",
            [],
        )
        .unwrap();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    assert_eq!(
        runtime.tick(&mut host).unwrap().ready_for_review,
        vec![done]
    );
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    assert_eq!(
        scalar(&fixture.db("solver"), "SELECT COUNT(*) FROM invocations"),
        1
    );
    assert_eq!(
        scalar(&fixture.db("runtime"), "SELECT COUNT(*) FROM runtime_jobs"),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn live_enqueue_survives_idle_and_deduplicates_even_after_ack() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert!(runtime.tick(&mut host).unwrap().ready_for_review.is_empty());
    let event = json!({"id":"live-task","subject":"live-task","reference":"task:live","payload":{"values":[3,7,12],"target":6.5}});
    let output = fixture.enqueue(event.clone());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let done = runtime.tick(&mut host).unwrap().ready_for_review.remove(0);
    assert_eq!(
        host.store()
            .get(&done.candidate_version_ids[0])
            .unwrap()
            .unwrap()
            .payload["value"],
        7.0
    );
    assert!(fixture.enqueue(event.clone()).status.success());
    assert!(runtime.tick(&mut host).unwrap().ready_for_review.is_empty());
    let mut changed = event;
    changed["payload"]["target"] = json!(1);
    assert!(!fixture.enqueue(changed).status.success());
    assert_eq!(
        scalar(
            &fixture.db("input"),
            "SELECT COUNT(*) FROM events WHERE ack=0"
        ),
        0
    );
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 2);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn fanout_and_backpressure_preserve_all_route_jobs_and_bound_the_queue() {
    let fixture = Fixture::new();
    let mut profile = fixture.read("input-profile.json");
    let event = profile["events"][0].clone();
    for index in 1..3 {
        let mut next = event.clone();
        next["id"] = json!(format!("extra-task-{index}"));
        next["reference"] = json!(format!("extra-task:{index}"));
        next["subject"] = json!(format!("extra-task-{index}"));
        profile["events"].as_array_mut().unwrap().push(next);
    }
    fixture.write("input-profile.json", profile);
    let mut config = fixture.read("runtime.json");
    let mut model = config["routes"][0].clone();
    model["id"] = json!("model");
    model["reasoner"]["adapter_id"] = json!("example.model-fixture");
    model["method"] = json!("learned");
    model["budget"]["max_iterations"] = Value::Null;
    config["routes"].as_array_mut().unwrap().push(model);
    fixture.write("runtime.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let mut done = Vec::new();
    let mut pressure = false;
    for _ in 0..10 {
        let tick = runtime.tick(&mut host).unwrap();
        pressure |= tick.backpressure;
        done.extend(tick.ready_for_review);
        assert!(runtime.report(0).unwrap().pending_jobs <= 2);
    }
    assert!(pressure);
    assert_eq!(done.len(), 6);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert_eq!(runtime.completed_after(0, 10).unwrap(), done);
    assert_eq!(
        scalar(&fixture.db("solver"), "SELECT COUNT(*) FROM invocations"),
        3
    );
    assert_eq!(
        scalar(&fixture.db("model"), "SELECT COUNT(*) FROM invocations"),
        3
    );
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn invalid_budget_is_rejected_before_ingress_and_changed_definition_cannot_reuse_identity() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut config = fixture.config();
    config.routes[0].budget.max_iterations = Some(999_999);
    assert!(TaskRuntime::open(&host, config).is_err());
    assert_eq!(
        scalar(
            &fixture.db("input"),
            "SELECT COUNT(*) FROM events WHERE ack=1"
        ),
        0
    );
    let runtime = fixture.runtime(&host);
    drop(runtime);
    let mut config = fixture.config();
    config.routes[0].objective = "Changed meaning".into();
    assert!(TaskRuntime::open(&host, config).is_err());
    let mut config = fixture.config();
    config.journal = fixture.directory.join("db/other-runtime.sqlite");
    assert!(TaskRuntime::open(&host, config).is_err());
    assert_eq!(
        scalar(
            &fixture.db("input"),
            "SELECT COUNT(*) FROM events WHERE ack=1"
        ),
        0
    );
    host.shutdown().unwrap();
}

#[test]
fn one_runtime_journal_has_one_owner_and_the_lock_is_released_on_drop() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let runtime = fixture.runtime(&host);
    assert!(TaskRuntime::open(&host, fixture.config()).is_err());
    drop(runtime);
    let _runtime = fixture.runtime(&host);
    host.shutdown().unwrap();
}

#[test]
fn reasoning_error_is_persisted_and_does_not_advance_to_review() {
    let fixture = Fixture::new();
    let mut config = fixture.read("runtime.json");
    config["routes"][0]["payload_fields"] = json!(["missing"]);
    fixture.write("runtime.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    assert!(runtime.tick(&mut host).is_err());
    let report = runtime.report(1).unwrap();
    assert_eq!(report.pending_jobs, 1);
    assert_eq!(report.failed_jobs, 1);
    assert_eq!(report.ready_for_review, 0);
    let error: String = fixture
        .db("runtime")
        .query_row("SELECT error FROM runtime_jobs", [], |row| row.get(0))
        .unwrap();
    assert!(error.contains("missing"));
    assert_eq!(
        scalar(&fixture.db("solver"), "SELECT COUNT(*) FROM invocations"),
        0
    );
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn continuous_cli_accepts_an_input_after_idle_and_stops_via_file_then_restarts_cleanly() {
    let fixture = Fixture::new();
    let stop = fixture.directory.join("stop");
    let child = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .arg("--config")
        .arg(fixture.directory.join("host.json"))
        .arg("--runtime")
        .arg(fixture.directory.join("runtime.json"))
        .arg("--stop-file")
        .arg(&stop)
        .current_dir(std::env::temp_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    wait_for(|| {
        fixture.directory.join("db/runtime.sqlite").exists()
            && fixture
                .db("runtime")
                .query_row(
                    "SELECT COUNT(*) FROM runtime_jobs WHERE state='ready_for_review'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                == 1
    });
    let event = json!({"id":"later","subject":"later","reference":"task:later","payload":{"values":[10,20],"target":19}});
    assert!(fixture.enqueue(event).status.success());
    wait_for(|| {
        scalar(
            &fixture.db("runtime"),
            "SELECT COUNT(*) FROM runtime_jobs WHERE state='ready_for_review'",
        ) == 2
    });
    std::fs::write(&stop, "").unwrap();
    wait_for(|| child.0.try_wait().unwrap().is_some());
    assert!(child.0.wait().unwrap().success());
    let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .arg("--config")
        .arg(fixture.directory.join("host.json"))
        .arg("--runtime")
        .arg(fixture.directory.join("runtime.json"))
        .args(["--ticks", "2"])
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ready_for_review"], 2);
    assert_eq!(report["pending_jobs"], 0);
    assert_eq!(
        scalar(&fixture.db("solver"), "SELECT COUNT(*) FROM invocations"),
        2
    );
}
fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(Instant::now() < deadline, "runtime did not make progress");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn an_observation_can_drive_the_same_reasoning_route_without_input_specific_logic() {
    let fixture = Fixture::new();
    // A protocol peer supplies a domain task as an observation. The runtime sees
    // the registered role and persisted evidence, not a concrete driver type.
    let script = r#"sequence=0
sent=0
while IFS= read -r line; do
 sequence=$((sequence+1))
 case "$line" in
 *'"type":"hello"'*) printf '{"protocol_version":1,"request_id":"stdio-%s","message":{"type":"manifest","manifest":{"protocol_version":1,"adapter_id":"example.input","adapter_version":"1","capabilities":[{"id":"task.submit","contract":{"kind":"observe","emits":["arcs.task.nearest.v1"]},"required_permissions":["input.read"]}]}}}\n' "$sequence" ;;
 *'"type":"poll"'*) if [ "$sent" = 0 ]; then
 printf '{"protocol_version":1,"request_id":"stdio-%s","message":{"type":"observation","message_id":"seen-1","observation":{"capability_id":"task.submit","external_subject":"observed-task","external_reference":"seen:1","payload":{"values":[1,2,4],"target":1.8}}}}\n' "$sequence"
 else printf '{"protocol_version":1,"request_id":"stdio-%s","message":{"type":"idle"}}\n' "$sequence"; fi ;;
 *'"type":"ack"'*) sent=1; printf '{"protocol_version":1,"request_id":"stdio-%s","message":{"type":"acknowledged","message_id":"seen-1"}}\n' "$sequence" ;;
 *'"type":"shutdown"'*) printf '{"protocol_version":1,"request_id":"stdio-%s","message":{"type":"bye"}}\n' "$sequence"; exit ;;
 esac
done
"#;
    let path = fixture.directory.join("observer.sh");
    std::fs::write(&path, script).unwrap();
    let mut config = fixture.read("host.json");
    config["adapters"][0] = json!({"program":"/bin/sh","args":[{"path":"observer.sh"}],"grant_file":"input-grant.json","timeout_ms":15000});
    fixture.write("host.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let tick = runtime.tick(&mut host).unwrap();
    assert_eq!(tick.ready_for_review.len(), 1);
    assert_eq!(host.report().unwrap().world_revision, 1);
    assert!(runtime.tick(&mut host).unwrap().ready_for_review.is_empty());
    let candidate = host
        .store()
        .get(&tick.ready_for_review[0].candidate_version_ids[0])
        .unwrap()
        .unwrap();
    assert_eq!(candidate.payload["value"], 2.0);
    host.shutdown().unwrap();
}

#[test]
fn a_queue_cannot_be_attached_to_a_different_core_database() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let runtime = fixture.runtime(&host);
    drop(runtime);
    host.shutdown().unwrap();
    drop(host);
    let mut config = fixture.read("host.json");
    config["database"] = json!("db/other-core.sqlite");
    fixture.write("host.json", config);
    let mut host = fixture.host();
    assert!(TaskRuntime::open(&host, fixture.config()).is_err());
    host.shutdown().unwrap();
}

#[test]
fn a_runtime_instance_cannot_poll_a_different_host_store() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let other = Fixture::new();
    let mut other_host = other.host();
    assert!(runtime.tick(&mut other_host).is_err());
    assert_eq!(
        scalar(
            &other.db("input"),
            "SELECT COUNT(*) FROM events WHERE ack=1"
        ),
        0
    );
    host.shutdown().unwrap();
    other_host.shutdown().unwrap();
}

#[test]
fn a_legacy_review_journal_is_migrated_without_recomputing_or_changing_completed_jobs() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = runtime.tick(&mut host).unwrap().ready_for_review;
    let artifacts = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    fixture.db("runtime").execute_batch("BEGIN IMMEDIATE;
        DROP INDEX runtime_pending;
        ALTER TABLE runtime_jobs RENAME TO new_jobs;
        CREATE TABLE runtime_jobs(sequence INTEGER PRIMARY KEY AUTOINCREMENT,route_id TEXT NOT NULL,input_version TEXT NOT NULL,source_sequence INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('pending','running','failed','ready_for_review')),error TEXT,result_json TEXT,UNIQUE(route_id,input_version));
        INSERT INTO runtime_jobs SELECT sequence,route_id,input_version,source_sequence,state,error,result_json FROM new_jobs;
        DROP TABLE new_jobs;
        CREATE INDEX runtime_pending ON runtime_jobs(state,sequence);
        COMMIT;").unwrap();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    assert_eq!(runtime.completed_after(0, 10).unwrap(), done);
    assert!(runtime.tick(&mut host).unwrap().ready_for_review.is_empty());
    assert_eq!(host.report().unwrap().stored_artifacts, artifacts);
    assert_eq!(runtime.report(0).unwrap().completed_goals, 0);
    host.shutdown().unwrap();
}

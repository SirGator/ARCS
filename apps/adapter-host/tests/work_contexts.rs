//! Nachweis der Arbeitskontexte: ein Core, mehrere Tätigkeiten, gebundene Ergebnisse.
mod support;
use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-work-context-{}-{}",
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
        fixture.write("host.json", json!({"config_version":1,"database":"db/core.sqlite","schemas":[root.join("adapters/example-reasoner/schemas/task.schema.json"),root.join("adapters/example-reasoner/schemas/candidate.schema.json")],"adapters":[
            {"program":support::adapter_binary("arcs-json-input"),"profile":"input-profile.json","args":[{"path":"db/input.sqlite"}],"grant_file":"input-grant.json","timeout_ms":15000},
            {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"solver-profile.json","args":[{"path":"db/solver.sqlite"}],"grant_file":"solver-grant.json","timeout_ms":15000},
            {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"model-profile.json","args":[{"path":"db/model.sqlite"}],"grant_file":"model-grant.json","timeout_ms":15000}]}));
        fixture.write("runtime.json", json!({"config_version":1,"runtime_id":"work-context-runtime","journal":"db/runtime.sqlite","interval_ms":5,"max_events_per_tick":1000,"max_jobs_per_tick":1,"max_pending_jobs":4,"routes":[{
            "id":"solve","source":{"adapter_id":"example.input","capability_id":"task.input"},"input_schema_id":"arcs.task.nearest.v1","payload_fields":["values","target"],
            "reasoner":{"adapter_id":"example.solver","capability_id":"solution.propose"},"method":"solver","objective":"Find the nearest listed value","target_schema_id":"arcs.solution_candidate.nearest.v1","allowed_capabilities":[],"constraints":{},
            "budget":{"max_context_items":1,"max_context_bytes":8192,"max_output_bytes":4096,"max_candidates":1,"max_runtime_ms":1000,"max_iterations":4096,"max_output_tokens":null}}]}));
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
    fn enqueue(&self, event: Value) -> std::process::Output {
        self.write("event.json", event);
        std::process::Command::new(support::adapter_binary("arcs-json-input"))
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

fn tick_until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    done: impl Fn(&arcs_host::task_runtime::TickReport) -> bool,
) -> arcs_host::task_runtime::TickReport {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        if done(&tick) {
            return tick;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// Liest den gespeicherten Arbeitskontext einer Aufgabe aus dem Core-Store.
fn context_of(host: &Host, sequence: u64) -> arcs::runtime::work_context::WorkContext {
    let subject = arcs::core::SubjectId(format!("work-context:work-context-runtime:{sequence}"));
    // Der Kontext liegt im Current-State seines Subjects.
    assert!(
        host.store()
            .current(
                &subject,
                &arcs::core::SchemaId("arcs.work_context.v1".into())
            )
            .unwrap()
            .is_some()
    );
    arcs::runtime::work_context::WorkContextService::new(
        host.store(),
        host.schemas(),
        &arcs::core::SystemClock,
    )
    .current("work-context-runtime", sequence)
    .unwrap()
    .expect("parsed work context")
}

#[test]
fn a_task_binds_its_result_to_a_persistent_work_context() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let tick = tick_until(&mut host, &mut runtime, |tick| {
        !tick.ready_for_review.is_empty()
    });
    let done = &tick.ready_for_review[0];

    let context = context_of(&host, done.sequence);
    assert_eq!(context.route_id, "solve");
    assert_eq!(context.state.as_str(), "ready_for_review");
    assert_eq!(context.task_sequence, done.sequence);
    assert!(
        context.artifact_versions.contains(&done.input_version_id),
        "the context must reference the task input"
    );
    assert_eq!(context.operations.len(), 1);
    let operation = &context.operations[0];
    assert_eq!(operation.kind, "reasoning");
    assert_eq!(operation.state.as_str(), "succeeded");
    assert_eq!(
        operation.version_id.as_ref(),
        Some(&done.result_version_id),
        "the result must be bound to the context"
    );

    // Der Statusbericht nennt denselben Stand ohne Nutzdaten.
    let reports = runtime.work_contexts(&host).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].task_sequence, done.sequence);
    assert_eq!(reports[0].state, "ready_for_review");
    assert_eq!(reports[0].operations.len(), 1);
    assert_eq!(reports[0].classification.as_str(), "internal");
    host.shutdown().unwrap();
}

#[test]
fn a_work_context_references_artifacts_without_copying_their_payload() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let tick = tick_until(&mut host, &mut runtime, |tick| {
        !tick.ready_for_review.is_empty()
    });
    let done = &tick.ready_for_review[0];

    let subject = arcs::core::SubjectId(format!(
        "work-context:work-context-runtime:{}",
        done.sequence
    ));
    let artifact = host
        .store()
        .current(
            &subject,
            &arcs::core::SchemaId("arcs.work_context.v1".into()),
        )
        .unwrap()
        .unwrap();
    // Der Kontext nennt nur Versionen. Nutzdaten des Auftrags bleiben im
    // Quellartefakt und werden nicht in den Zustand kopiert.
    assert!(artifact.payload.get("values").is_none());
    assert!(artifact.payload.get("target").is_none());
    assert_eq!(
        artifact.payload["artifact_versions"],
        json!([done.input_version_id.0])
    );
    host.shutdown().unwrap();
}

#[test]
fn two_tasks_keep_separate_contexts_in_one_core() {
    let fixture = Fixture::new();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let first = tick_until(&mut host, &mut runtime, |tick| {
        !tick.ready_for_review.is_empty()
    })
    .ready_for_review
    .remove(0);

    let event = json!({"id":"second-task","subject":"second-task","reference":"task:second","payload":{"values":[1,2,3],"target":2.5}});
    let output = fixture.enqueue(event);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let second = tick_until(&mut host, &mut runtime, |tick| {
        !tick.ready_for_review.is_empty()
    })
    .ready_for_review
    .remove(0);

    assert_ne!(first.sequence, second.sequence);
    let one = context_of(&host, first.sequence);
    let two = context_of(&host, second.sequence);
    assert_ne!(one.context_id, two.context_id);
    assert!(one.artifact_versions.contains(&first.input_version_id));
    assert!(!one.artifact_versions.contains(&second.input_version_id));
    assert!(two.artifact_versions.contains(&second.input_version_id));
    assert!(!two.artifact_versions.contains(&first.input_version_id));
    // Beide Tätigkeiten laufen im selben Core und im selben Store.
    assert_eq!(runtime.work_contexts(&host).unwrap().len(), 2);
    host.shutdown().unwrap();
}

#[test]
fn a_restart_continues_the_persisted_context_instead_of_restarting_it() {
    let fixture = Fixture::new();
    let before = {
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        let tick = tick_until(&mut host, &mut runtime, |tick| {
            !tick.ready_for_review.is_empty()
        });
        let done = &tick.ready_for_review[0];
        let before = context_of(&host, done.sequence);
        host.shutdown().unwrap();
        before
    };

    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let after = context_of(&host, before.task_sequence);
    assert_eq!(after.context_id, before.context_id);
    assert_eq!(after.version, before.version);
    assert_eq!(after.state.as_str(), "ready_for_review");
    assert_eq!(after.operations.len(), 1);

    // Ein weiterer Tick legt keinen zweiten Kontext an.
    runtime.tick(&mut host).unwrap();
    let repeated = context_of(&host, before.task_sequence);
    assert_eq!(repeated.context_id, before.context_id);
    assert_eq!(repeated.version, before.version);
    host.shutdown().unwrap();
}

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arcs::action::ActionService;
use arcs::adapters::{AdapterId, CapabilityRef};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SystemClock, VersionId};
use arcs::decision::proposal::{CandidateProposal, ProposalService};
use arcs::execution::ExecutionError;
use arcs::store::SqliteArtifactStore;
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs_host::{Host, HostError};
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    config_path: PathBuf,
    config: Value,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-configured-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let base = directory.join("config with spaces");
        for path in [
            &base,
            &base.join("schemas"),
            &base.join("grants"),
            &directory.join("bin"),
        ] {
            std::fs::create_dir(path).unwrap();
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut config: Value = serde_json::from_slice(
            &std::fs::read(root.join("apps/adapter-host/config/host.json")).unwrap(),
        )
        .unwrap();
        config["database"] = json!("../data/core.sqlite");
        config["node_id"] = json!("configured-test");
        for schema in config["schemas"].as_array_mut().unwrap() {
            let path = Path::new(schema.as_str().unwrap());
            let name = path.file_name().unwrap();
            std::fs::copy(
                root.join("apps/adapter-host/config").join(path),
                base.join("schemas").join(name),
            )
            .unwrap();
            *schema = json!(format!("schemas/{}", name.to_str().unwrap()));
        }
        for (index, adapter) in ["sensor", "fan"].into_iter().enumerate() {
            let filename = format!("arcs-simulated-{adapter}{}", std::env::consts::EXE_SUFFIX);
            let target = support::adapter_binary(&format!("arcs-simulated-{adapter}"));
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, directory.join("bin").join(&filename)).unwrap();
            #[cfg(not(unix))]
            std::fs::copy(target, directory.join("bin").join(&filename)).unwrap();
            config["adapters"][index]["program"] = json!(format!("../bin/{filename}"));
            config["adapters"][index]["profile"] = json!(format!("{adapter} profile.json"));
            config["adapters"][index]["working_directory"] = json!(".");
            std::fs::copy(
                root.join(format!("adapters/simulated-{adapter}/config/profile.json")),
                base.join(format!("{adapter} profile.json")),
            )
            .unwrap();
            std::fs::copy(
                root.join(format!("apps/adapter-host/config/grants/{adapter}.json")),
                base.join(format!("grants/{adapter}.json")),
            )
            .unwrap();
        }
        config["adapters"][1]["args"][0]["path"] = json!("../data/adapter.sqlite");
        let fixture = Self {
            config_path: base.join("host.json"),
            directory,
            config,
        };
        fixture.save();
        fixture
    }

    fn save(&self) {
        std::fs::write(&self.config_path, serde_json::to_vec(&self.config).unwrap()).unwrap();
    }

    fn base(&self) -> &Path {
        self.config_path.parent().unwrap()
    }

    fn run(&self) -> Output {
        // Die Pfade müssen von der Konfigurationsdatei, nicht vom CWD abhängen.
        Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(&self.config_path)
            .current_dir(&self.directory)
            .output()
            .unwrap()
    }

    fn run_ok(&self) -> Value {
        let output = self.run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn one_configuration_connects_both_adapters_and_replays_without_new_effects() {
    let fixture = Fixture::new();
    let first = fixture.run_ok();
    assert_eq!(first["adapters"].as_array().unwrap().len(), 2);
    assert_eq!(first["adapters"][0]["observations_committed"], 2);
    assert_eq!(first["adapters"][1]["observations_committed"], 3);
    assert_eq!(first["estimates"].as_array().unwrap().len(), 3);
    assert_eq!(
        first["adapters"][1]["requests"][0]["payload"],
        json!({"value":31.0,"unit":"Cel"})
    );
    assert_eq!(
        first["adapters"][1]["requests"][1]["payload"]["enabled"],
        false
    );
    assert_eq!(
        first["adapters"][1]["requests"][1]["payload"]["actuation_count"],
        0
    );
    assert!(fixture.directory.join("data/adapter.sqlite").is_file());
    let second = fixture.run_ok();
    assert_eq!(second["stored_artifacts"], first["stored_artifacts"]);
    assert_eq!(second["world_revision"], first["world_revision"]);
    assert_eq!(second["estimates"], first["estimates"]);
    assert_eq!(second["adapters"][0]["observations_replayed"], 2);
    assert_eq!(second["adapters"][0]["observations_committed"], 0);
    assert_eq!(
        second["adapters"][1]["requests"],
        first["adapters"][1]["requests"]
    );
    let store = SqliteArtifactStore::open(fixture.directory.join("data/core.sqlite")).unwrap();
    assert!(store.committed_after(0).unwrap().iter().all(|c| !matches!(
        c.artifact.artifact_type.as_str(),
        "action" | "approval" | "execution_result"
    )));
}

#[test]
fn static_configuration_errors_do_not_create_a_database() {
    let mut fixture = Fixture::new();
    let valid = fixture.config.clone();
    let mut cases = Vec::new();
    let mut unknown = valid.clone();
    unknown["unexpected"] = json!(true);
    cases.push(unknown);
    let mut version = valid.clone();
    version["config_version"] = json!(2);
    cases.push(version);
    let mut timeout = valid.clone();
    timeout["adapters"][0]["timeout_ms"] = json!(0);
    cases.push(timeout);
    let mut duplicate = valid.clone();
    duplicate["adapters"][1] = duplicate["adapters"][0].clone();
    cases.push(duplicate);
    let mut bad_request = valid.clone();
    bad_request["adapters"][1]["requests"][0]["payload"] = json!({"unknown":1});
    cases.push(bad_request);
    let mut bad_path_arg = valid.clone();
    bad_path_arg["adapters"][1]["args"][0]["unexpected"] = json!(1);
    cases.push(bad_path_arg);
    let mut missing = valid.clone();
    missing["adapters"][1]["grant_file"] = json!("missing.json");
    cases.push(missing);
    let mut unknown_schema = valid.clone();
    unknown_schema["adapters"][1]["requests"][0]["response_schema_id"] = json!("arcs.missing.v1");
    cases.push(unknown_schema);
    for config in cases {
        fixture.config = config;
        fixture.save();
        let output = fixture.run();
        assert!(!output.status.success());
        assert!(
            !fixture.directory.join("data").exists(),
            "invalid configuration created runtime state"
        );
    }
}

#[test]
fn changed_request_definition_requires_a_new_request_id() {
    let mut fixture = Fixture::new();
    let initial = fixture.run_ok();
    fixture.config["adapters"][1]["requests"][0]["subject"] = json!("another-subject");
    fixture.save();
    let conflict = fixture.run();
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("IdentityConflict"));
    let store = SqliteArtifactStore::open(fixture.directory.join("data/core.sqlite")).unwrap();
    assert_eq!(
        store.len().unwrap(),
        initial["stored_artifacts"].as_u64().unwrap()
    );
    fixture.config["adapters"][1]["requests"][0]["id"] = json!("fresh-temperature-2");
    fixture.save();
    let next = fixture.run_ok();
    assert!(
        next["stored_artifacts"].as_u64().unwrap() > initial["stored_artifacts"].as_u64().unwrap()
    );
    assert_ne!(
        next["adapters"][1]["requests"][0]["version_id"],
        initial["adapters"][1]["requests"][0]["version_id"]
    );
}

#[test]
fn replacing_a_bound_schema_is_rejected_before_ingest() {
    let fixture = Fixture::new();
    let initial = fixture.run_ok();
    let path = fixture.base().join("schemas/temperature.schema.json");
    let mut schema: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    schema["properties"]["value"]["minimum"] = json!(0);
    std::fs::write(path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let error = fixture.run();
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("SchemaDrift"));
    let store = SqliteArtifactStore::open(fixture.directory.join("data/core.sqlite")).unwrap();
    assert_eq!(
        store.len().unwrap(),
        initial["stored_artifacts"].as_u64().unwrap()
    );
}

#[test]
fn configured_budget_limits_each_adapter_per_round() {
    let mut fixture = Fixture::new();
    fixture.config["adapters"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    fixture.config["polling"] =
        json!({"rounds":2,"interval_ms":0,"max_observations_per_adapter":1});
    fixture.save();
    let result = fixture.run_ok();
    assert_eq!(result["adapters"][0]["poll_limit_hits"], 2);
    assert_eq!(result["adapters"][0]["observations_committed"], 2);
    assert_eq!(result["estimates"][0]["value"]["value"], 26.0);
}

#[test]
fn request_configuration_cannot_dispatch_an_act_capability() {
    let mut fixture = Fixture::new();
    fixture.config["adapters"][1]["requests"][0] = json!({
        "id":"not-an-action","capability_id":"fan.set","input_schema_id":"arcs.fan_candidate.v1",
        "response_schema_id":"arcs.execution_result.fan.v1","subject":"fan-01","payload":{"device_id":"fan-01","enabled":true}
    });
    fixture.save();
    let error = fixture.run();
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("not a Request capability"));
    let store = SqliteArtifactStore::open(fixture.directory.join("data/core.sqlite")).unwrap();
    assert!(store.is_empty().unwrap());
}

struct Pass;
impl Verifier for Pass {
    fn verify(&self, _artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: VerificationVerdict::Pass,
            findings: vec![],
        })
    }
}

#[test]
fn application_can_execute_only_materialized_approved_actions_through_generic_host() {
    let fixture = Fixture::new();
    let mut host = Host::open(&fixture.config_path).unwrap();
    let adapter = AdapterId("demo.fan".into());
    let missing = host.execute_approved(&adapter, &VersionId("missing-action".into()));
    assert!(matches!(
        missing,
        Err(HostError::Execution {
            source: ExecutionError::MissingAction(_),
            ..
        })
    ));
    let temperature = host.request("fan-temperature-1").unwrap();
    let mut ids = host
        .store()
        .durable_id_generator("test-application")
        .unwrap();
    let candidate = ProposalService::new(
        host.registry(),
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .propose(CandidateProposal {
        proposal_id: "configured-action-1".into(),
        rule_id: "test.application.v1".into(),
        schema_id: SchemaId("arcs.fan_candidate.v1".into()),
        payload: json!({"device_id":"fan-01","enabled":true}),
        evidence: vec![temperature.version_id],
        required_capabilities: vec![CapabilityRef::new("demo.fan", "fan.set")],
    })
    .unwrap();
    assert!(matches!(
        host.execute_approved(&adapter, &candidate.version_id),
        Err(HostError::Execution {
            source: ExecutionError::NotActionArtifact,
            ..
        })
    ));
    let report = VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .verify(&candidate.version_id, &Pass)
        .unwrap();
    let approval = ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .decide(
            &candidate.version_id,
            &report.version_id,
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::System,
                id: "test.application".into(),
            },
            "simulated test action".into(),
        )
        .unwrap();
    let action = ActionService::new(
        host.registry(),
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .materialize(
        &approval.version_id,
        &CapabilityRef::new("demo.fan", "fan.set"),
    )
    .unwrap();
    let first = host.execute_approved(&adapter, &action.version_id).unwrap();
    assert_eq!(
        host.execute_approved(&adapter, &action.version_id).unwrap(),
        first
    );
    let readback = host.request("fan-state-1").unwrap();
    assert_eq!(readback.payload["enabled"], true);
    assert_eq!(readback.payload["actuation_count"], 1);
    host.poll_round().unwrap();
    assert!(
        host.world()
            .estimates()
            .any(|(_, e)| e.schema_id().0 == "arcs.state.fan.v1" && e.value()["enabled"] == true)
    );
    host.shutdown().unwrap();
}

#[cfg(unix)]
#[test]
fn timeout_from_configuration_is_applied_to_the_child() {
    let mut fixture = Fixture::new();
    fixture.config["adapters"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    fixture.config["adapters"][0]["program"] = json!("/bin/sh");
    fixture.config["adapters"][0]["profile"] = Value::Null;
    fixture.config["adapters"][0]["args"] = json!(["-c", "exec sleep 10"]);
    fixture.config["adapters"][0]["timeout_ms"] = json!(150);
    fixture.save();
    let started = Instant::now();
    let result = fixture.run();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Timeout"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

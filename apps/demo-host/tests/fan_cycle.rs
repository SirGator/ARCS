mod support;

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arcs::adapters::stdio::StdioAdapter;
use arcs::adapters::stdio::protocol::{
    AdapterMessage, Envelope, HostMessage, WIRE_VERSION, read_frame, write_frame,
};
use arcs::adapters::{
    ActAdapter, ActInvocation, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    AdapterRegistry, AdapterRegistryError, CapabilityContract, CapabilityDescriptor, CapabilityId,
    CapabilityRef, ProducerClass,
};
use arcs::core::{
    ActorType, SchemaId, SchemaRegistry, SourceKind, SubjectId, SystemClock, TrustLevel, VersionId,
};
use arcs::execution::{ExecutionError, ExecutionService};
use arcs::request::{RequestAdapter, RequestInvocation};
use arcs::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use rusqlite::Connection;
use serde_json::{Value, json};

struct Harness {
    directory: PathBuf,
}

impl Harness {
    fn new(raw_temperature: f64) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-fan-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let h = Self { directory };
        let mut profile: Value = serde_json::from_str(arcs_simulated_fan::DEFAULT_PROFILE).unwrap();
        profile["raw_temperature"] = json!(raw_temperature);
        std::fs::write(
            h.path("profile.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        h
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn run(&self, cycle: &str, reject: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_arcs-fan-demo"));
        command
            .arg(support::adapter_binary("arcs-simulated-fan"))
            .arg(self.path("core.sqlite"))
            .arg(self.path("adapter.sqlite"))
            .arg(self.path("profile.json"))
            .arg(cycle);
        if reject {
            command.arg("--reject");
        }
        command.output().unwrap()
    }

    fn run_ok(&self, cycle: &str, reject: bool) -> Value {
        let output = self.run(cycle, reject);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn simulator(&self, database: &Path) -> Command {
        let mut command = Command::new(support::adapter_binary("arcs-simulated-fan"));
        command.arg(self.path("profile.json")).arg(database);
        command
    }

    fn schemas(&self) -> SchemaRegistry {
        let database = Connection::open(self.path("core.sqlite")).unwrap();
        let mut query = database
            .prepare("SELECT schema_json FROM schema_bindings")
            .unwrap();
        let mut schemas = SchemaRegistry::new();
        for document in query.query_map([], |row| row.get::<_, String>(0)).unwrap() {
            schemas.register_json(&document.unwrap()).unwrap();
        }
        schemas
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn manifest() -> AdapterManifest {
    let capability = |name: &str, contract, permission: &str| CapabilityDescriptor {
        id: CapabilityId(name.into()),
        contract,
        required_permissions: vec![permission.into()],
    };
    AdapterManifest {
        protocol_version: 1,
        adapter_id: AdapterId("demo.fan".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            capability(
                "temperature.read",
                CapabilityContract::Request {
                    accepts: vec![SchemaId("arcs.request.temperature.v1".into())],
                    emits: vec![SchemaId("arcs.measurement.temperature.v1".into())],
                },
                "sensor.read",
            ),
            capability(
                "fan.read",
                CapabilityContract::Request {
                    accepts: vec![SchemaId("arcs.request.fan.v1".into())],
                    emits: vec![SchemaId("arcs.state.fan.v1".into())],
                },
                "sensor.read",
            ),
            capability(
                "temperature.observe",
                CapabilityContract::Observe {
                    emits: vec![SchemaId("arcs.measurement.temperature.v1".into())],
                },
                "sensor.read",
            ),
            capability(
                "fan.observe",
                CapabilityContract::Observe {
                    emits: vec![SchemaId("arcs.state.fan.v1".into())],
                },
                "sensor.read",
            ),
            capability(
                "fan.set",
                CapabilityContract::Act {
                    accepts: vec![SchemaId("arcs.fan_candidate.v1".into())],
                    emits: vec![SchemaId("arcs.execution_result.fan.v1".into())],
                    idempotent: true,
                },
                "fan.write",
            ),
        ],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("demo.fan".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: manifest().capabilities.into_iter().map(|c| c.id).collect(),
        granted_permissions: vec!["sensor.read".into(), "fan.write".into()],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        data_limits: None,
        reasoning_limits: None,
    }
}

fn journal_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM invocations", [], |row| row.get(0))
        .unwrap()
}

fn action_invocation(h: &Harness, output: &Value) -> ActInvocation {
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let action = store
        .get(&VersionId(
            output["artifacts"]["action"].as_str().unwrap().into(),
        ))
        .unwrap()
        .unwrap();
    let execution = store
        .get(&VersionId(
            output["artifacts"]["execution"].as_str().unwrap().into(),
        ))
        .unwrap()
        .unwrap();
    ActInvocation {
        invocation_id: execution
            .stream_key
            .strip_prefix("execution:")
            .unwrap()
            .into(),
        capability: CapabilityRef::new("demo.fan", "fan.set"),
        action_version_id: action.version_id,
        action_schema_id: action.schema_id,
        payload: action.payload,
        result_schema_id: execution.schema_id,
    }
}

#[test]
fn authorized_cycle_has_audit_chain_world_readback_and_stable_replay() {
    let h = Harness::new(310.0);
    let first = h.run_ok("cycle-1", false);
    assert_eq!(first["status"], "executed");
    assert_eq!(first["temperature"]["value"], 31.0);
    assert_eq!(first["fan_state"]["enabled"], true);
    assert_eq!(first["fan_state"]["actuation_count"], 1);
    let estimates = first["world"].as_array().unwrap();
    assert!(
        estimates
            .iter()
            .any(|e| e["schema_id"] == "arcs.state.fan.v1" && e["value"]["enabled"] == true)
    );
    assert!(estimates.iter().any(
        |e| e["schema_id"] == "arcs.measurement.temperature.v1" && e["value"]["value"] == 31.0
    ));
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let id = |key: &str| VersionId(first["artifacts"][key].as_str().unwrap().into());
    for (from, to, relation) in [
        ("candidate", "temperature", relation_kinds::supported_by()),
        ("candidate", "cycle", relation_kinds::supported_by()),
        ("verification", "candidate", relation_kinds::verifies()),
        ("approval", "candidate", relation_kinds::approves()),
        ("action", "approval", relation_kinds::based_on()),
        ("execution", "action", relation_kinds::result_of()),
        ("outcome", "execution", relation_kinds::evaluates()),
        ("outcome", "readback", relation_kinds::supported_by()),
    ] {
        assert!(
            ArtifactRelations::new(&store)
                .outgoing(&id(from))
                .unwrap()
                .iter()
                .any(|r| r.to == id(to) && r.kind == relation)
        );
    }
    let candidate = store.get(&id("candidate")).unwrap().unwrap();
    assert_eq!(candidate.created_by.actor_type, ActorType::System);
    assert!(candidate.provenance.unwrap().models_used.is_empty());
    assert_eq!(
        store.get(&id("outcome")).unwrap().unwrap().payload["verdict"],
        "success"
    );
    assert_eq!(journal_count(&h.path("adapter.sqlite")), 3);
    assert_eq!(h.run_ok("cycle-1", false), first);
    assert_eq!(journal_count(&h.path("adapter.sqlite")), 3);
}

#[test]
fn cool_rejected_and_failed_verification_do_not_actuate() {
    for (raw, reject, expected) in [
        (200.0, false, "no_action"),
        (310.0, true, "rejected"),
        (1000.0, false, "verification_failed"),
    ] {
        let h = Harness::new(raw);
        let result = h.run_ok("cycle-1", reject);
        assert_eq!(result["status"], expected);
        assert_eq!(result["fan_state"]["enabled"], false);
        assert_eq!(result["fan_state"]["actuation_count"], 0);
        assert!(result["artifacts"].get("action").is_none());
        assert!(result["artifacts"].get("execution").is_none());
        assert_eq!(journal_count(&h.path("adapter.sqlite")), 2);
        assert_eq!(h.run_ok("cycle-1", reject), result);
    }
}

#[test]
fn changed_policy_needs_a_new_cycle_and_cannot_reuse_previous_approval() {
    let h = Harness::new(310.0);
    let initial = h.run_ok("cycle-1", false);
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(h.path("profile.json")).unwrap()).unwrap();
    config["max_auto_approve_celsius"] = json!(30.0);
    std::fs::write(h.path("profile.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    let conflict = h.run("cycle-1", false);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("IdentityConflict"));
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    assert_eq!(
        store.len().unwrap(),
        initial["stored_artifacts"].as_u64().unwrap()
    );
    assert_eq!(journal_count(&h.path("adapter.sqlite")), 3);
    let next = h.run_ok("cycle-2", false);
    assert_eq!(next["status"], "verification_failed");
    // Ablehnung setzt einen bereits laufenden Lüfter nicht stillschweigend zurück.
    assert_eq!(next["fan_state"]["actuation_count"], 1);
    assert_eq!(journal_count(&h.path("adapter.sqlite")), 5);
}

#[test]
fn disabled_action_capability_is_checked_even_for_completed_actions() {
    let h = Harness::new(310.0);
    let completed = h.run_ok("cycle-1", false);
    let schemas = h.schemas();
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let mut registry = AdapterRegistry::new();
    let mut restricted = grant();
    restricted.enabled_capabilities.retain(|c| c.0 != "fan.set");
    restricted.granted_permissions.retain(|p| p != "fan.write");
    let mut adapter = StdioAdapter::connect(
        &mut h.simulator(&h.path("adapter.sqlite")),
        Duration::from_secs(3),
        restricted,
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut ids = store.durable_id_generator("test").unwrap();
    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &SystemClock)
        .execute(
            &adapter,
            &VersionId(completed["artifacts"]["action"].as_str().unwrap().into()),
        );
    assert!(matches!(
        result,
        Err(ExecutionError::Authorization(
            AdapterRegistryError::CapabilityNotEnabled { .. }
        ))
    ));
    adapter.shutdown().unwrap();
    assert_eq!(journal_count(&h.path("adapter.sqlite")), 3);
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn adapter_journal_recovers_lost_execution_response_and_rejects_conflicting_reuse() {
    let h = Harness::new(310.0);
    let completed = h.run_ok("cycle-1", false);
    let invocation = action_invocation(&h, &completed);
    let adapter_db = h.path("lost-response.sqlite");
    let mut child = ChildGuard(
        h.simulator(&adapter_db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut stdin = child.0.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.0.stdout.take().unwrap());
    write_frame(
        &mut stdin,
        &Envelope {
            protocol_version: WIRE_VERSION,
            request_id: "hello".into(),
            message: HostMessage::Hello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<Envelope<AdapterMessage>>(&mut stdout)
            .unwrap()
            .message,
        AdapterMessage::Manifest { .. }
    ));
    write_frame(
        &mut stdin,
        &Envelope {
            protocol_version: WIRE_VERSION,
            request_id: "execute".into(),
            message: HostMessage::Execute {
                invocation: invocation.clone(),
            },
        },
    )
    .unwrap();
    // Den Commit beobachten, ohne die Execution-Antwort zu lesen.
    let deadline = Instant::now() + Duration::from_secs(3);
    while journal_count(&adapter_db) == 0 {
        assert!(
            Instant::now() < deadline,
            "simulated execution did not commit"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let schemas = h.schemas();
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let mut registry = AdapterRegistry::new();
    let adapter = StdioAdapter::connect(
        &mut h.simulator(&adapter_db),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let replay = ActAdapter::execute(&adapter, &invocation).unwrap();
    assert_eq!(replay.result_payload["result"]["enabled"], true);
    assert_eq!(replay.result_payload["result"]["actuation_count"], 1);
    assert_eq!(journal_count(&adapter_db), 1);
    let mut conflicting = invocation.clone();
    conflicting.payload["payload"]["enabled"] = json!(false);
    assert!(matches!(
        ActAdapter::execute(&adapter, &conflicting),
        Err(AdapterCallError::Rejected(_))
    ));
    let state: (bool, i64) = Connection::open(adapter_db)
        .unwrap()
        .query_row(
            "SELECT enabled, actuation_count FROM fan_state",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, (true, 1));
    let mut invalid = invocation;
    invalid.invocation_id = "invalid-empty-action".into();
    invalid.action_version_id.0.clear();
    let adapter = StdioAdapter::connect(
        &mut h.simulator(&h.path("lost-response.sqlite")),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    assert!(matches!(
        ActAdapter::execute(&adapter, &invalid),
        Err(AdapterCallError::Rejected(_))
    ));
    assert_eq!(journal_count(&h.path("lost-response.sqlite")), 1);
}

#[cfg(unix)]
fn fixture(second_response: &str) -> Command {
    let hello = json!({"protocol_version": 1, "request_id": "stdio-1", "message": {"type": "manifest", "manifest": manifest()}});
    let mut command = Command::new("sh");
    command.arg("-c").arg("IFS= read -r request || exit 1\nprintf '%s\\n' \"$1\"\nIFS= read -r request || exit 1\nprintf '%s\\n' \"$2\"")
        .arg("fan-fixture").arg(hello.to_string()).arg(second_response);
    command
}

#[cfg(unix)]
#[test]
fn request_transport_checks_nested_correlation_and_preserves_error_classes() {
    let h = Harness::new(310.0);
    h.run_ok("cycle-1", false);
    let schemas = h.schemas();
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let invocation = RequestInvocation {
        invocation_id: "request-test".into(),
        capability: CapabilityRef::new("demo.fan", "temperature.read"),
        request_version_id: VersionId("request-1".into()),
        request_schema_id: SchemaId("arcs.request.temperature.v1".into()),
        subject: SubjectId("temperature-01".into()),
        request_payload: json!({"device_id": "temperature-01"}),
        response_schema_id: SchemaId("arcs.measurement.temperature.v1".into()),
        max_response_bytes: 4096,
    };
    let wrong = json!({"protocol_version": 1, "request_id": "stdio-2", "message": {"type": "request_result", "response": {
        "invocation_id": "wrong", "external_reference": "test", "payload": {"value": 31.0, "unit": "Cel"}
    }}}).to_string();
    let temporary = json!({"protocol_version": 1, "request_id": "stdio-2", "message": {"type": "error", "code": "offline", "message": "temporary", "retryable": true}}).to_string();
    let permanent = json!({"protocol_version": 1, "request_id": "stdio-2", "message": {"type": "error", "code": "denied", "message": "permanent", "retryable": false}}).to_string();
    for (response, expected) in [
        (wrong, "protocol"),
        (temporary, "unavailable"),
        (permanent, "rejected"),
        ("{".into(), "protocol"),
    ] {
        let mut registry = AdapterRegistry::new();
        let adapter = StdioAdapter::connect(
            &mut fixture(&response),
            Duration::from_secs(3),
            grant(),
            &mut registry,
            &schemas,
            &store,
        )
        .unwrap();
        let result = RequestAdapter::fetch(&adapter, &invocation);
        assert!(
            match expected {
                "protocol" => matches!(result, Err(AdapterCallError::Protocol(_))),
                "unavailable" => matches!(result, Err(AdapterCallError::Unavailable(_))),
                "rejected" => matches!(result, Err(AdapterCallError::Rejected(_))),
                _ => false,
            },
            "{result:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn execution_transport_rejects_wrong_nested_invocation_id() {
    let h = Harness::new(310.0);
    let completed = h.run_ok("cycle-1", false);
    let invocation = action_invocation(&h, &completed);
    let schemas = h.schemas();
    let store = SqliteArtifactStore::open(h.path("core.sqlite")).unwrap();
    let mut registry = AdapterRegistry::new();
    let wrong = json!({"protocol_version": 1, "request_id": "stdio-2", "message": {"type": "execution_result", "response": {
        "invocation_id": "wrong", "external_reference": "test", "result_payload": {"ok": true}
    }}}).to_string();
    let adapter = StdioAdapter::connect(
        &mut fixture(&wrong),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    assert!(matches!(
        ActAdapter::execute(&adapter, &invocation),
        Err(AdapterCallError::Protocol(_))
    ));
}

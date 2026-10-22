mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use arcs::adapters::stdio::protocol::{
    AdapterMessage, Envelope, MAX_FRAME_BYTES, WIRE_VERSION, WireError, read_frame,
};
use arcs::adapters::stdio::{StdioAdapter, StdioError};
use arcs::adapters::{AdapterGrant, AdapterRegistry};
use arcs::core::{SchemaRegistry, SystemClock};
use arcs::store::{SqliteArtifactStore, StoreError};
use arcs::world::observation::{ObservationError, ObservationLog, ObservationService};
use arcs::world::{WorldReducer, WorldState};
use serde_json::{Value, json};

struct TemporaryFile(PathBuf);

impl TemporaryFile {
    fn new(extension: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(std::env::temp_dir().join(format!(
            "arcs-stdio-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        )))
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(arcs_simulated_sensor::TEMPERATURE_SCHEMA)
        .unwrap();
    schemas
}

fn grant() -> AdapterGrant {
    serde_json::from_str(include_str!("../config/sensor.grant.json")).unwrap()
}

fn profile() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters/simulated-sensor/config/profile.json")
}

fn sensor(profile: &std::path::Path) -> Command {
    let mut command = Command::new(support::adapter_binary("arcs-simulated-sensor"));
    command.arg(profile);
    command
}

#[test]
fn external_profile_to_world_survives_restart_and_accepts_new_measurements() {
    let database = TemporaryFile::new("sqlite");
    let run = |profile: &std::path::Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_arcs-adapter-demo"))
            .arg(support::adapter_binary("arcs-simulated-sensor"))
            .arg(&database.0)
            .arg(profile)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let initial = run(&profile());
    assert_eq!(initial["stored_artifacts"], 2);
    assert_eq!(initial["world_revision"], 2);
    assert_eq!(
        initial["estimates"][0]["value"],
        json!({"value": 26.0, "unit": "Cel"})
    );
    assert_eq!(run(&profile()), initial);

    let updated = TemporaryFile::new("json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(profile()).unwrap()).unwrap();
    config["samples"].as_array_mut().unwrap().push(json!({
        "id": "temperature-01:sample-3", "raw": {"temperature_raw": 300}
    }));
    std::fs::write(&updated.0, serde_json::to_vec(&config).unwrap()).unwrap();
    let latest = run(&updated.0);
    assert_eq!(latest["stored_artifacts"], 3);
    assert_eq!(latest["world_revision"], 3);
    assert_eq!(latest["estimates"][0]["value"]["value"], 30.0);
}

#[test]
fn conflicting_replayed_sample_is_rejected_without_changing_history() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut registry = AdapterRegistry::new();
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&profile()),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    let first = adapter.poll(&mut observations).unwrap().unwrap();
    adapter.shutdown().unwrap();
    assert_eq!(store.len().unwrap(), 1);

    let modified = TemporaryFile::new("json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(profile()).unwrap()).unwrap();
    config["samples"][0]["raw"]["temperature_raw"] = json!(999);
    std::fs::write(&modified.0, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&modified.0),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    assert!(matches!(
        adapter.poll(&mut observations),
        Err(StdioError::Observation(ObservationError::Store(
            StoreError::IdempotencyConflict
        )))
    ));
    assert_eq!(
        ObservationLog::new(&store).after(None).unwrap(),
        vec![first]
    );
}

#[test]
fn replay_of_older_measurement_does_not_roll_back_current_state() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut registry = AdapterRegistry::new();
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&profile()),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    let first = adapter.poll(&mut observations).unwrap().unwrap();
    let latest = adapter.poll(&mut observations).unwrap().unwrap();
    adapter.shutdown().unwrap();
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&profile()),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    assert_eq!(adapter.poll(&mut observations).unwrap().unwrap(), first);
    let current = store
        .current(
            latest.artifact().subject.as_ref().unwrap(),
            &latest.artifact().schema_id,
        )
        .unwrap()
        .unwrap();
    assert_eq!(&current, latest.artifact());
    let mut world = WorldState::new();
    for recorded in ObservationLog::new(&store).after(None).unwrap() {
        WorldReducer::new().reduce(&mut world, &recorded).unwrap();
    }
    assert_eq!(world.estimates().next().unwrap().1.value()["value"], 26.0);
}

#[test]
fn framing_rejects_oversize_unterminated_and_unknown_fields() {
    let mut oversized = std::io::Cursor::new(vec![b'x'; MAX_FRAME_BYTES + 100]);
    assert!(matches!(
        read_frame::<Value>(&mut oversized),
        Err(WireError::FrameTooLarge)
    ));
    assert_eq!(oversized.position(), MAX_FRAME_BYTES as u64 + 1);
    let mut truncated = std::io::Cursor::new(b"{\"protocol_version\":1}");
    assert!(matches!(
        read_frame::<Value>(&mut truncated),
        Err(WireError::Disconnected)
    ));
    let mut unknown = std::io::Cursor::new(b"{\"protocol_version\":1,\"request_id\":\"a\",\"message\":{\"type\":\"idle\"},\"trust\":\"high\"}\n");
    assert!(matches!(
        read_frame::<Envelope<AdapterMessage>>(&mut unknown),
        Err(WireError::Json(_))
    ));
}

#[test]
fn wrong_adapter_identity_and_invalid_measurement_never_commit() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    let mut wrong_grant = grant();
    wrong_grant.adapter_id.0 = "another.adapter".into();
    let result = StdioAdapter::connect(
        &mut sensor(&profile()),
        Duration::from_secs(3),
        wrong_grant,
        &mut registry,
        &schemas,
        &store,
    );
    assert!(matches!(
        result,
        Err(StdioError::Registration(
            arcs::adapters::AdapterRegistryError::GrantAdapterMismatch
        ))
    ));
    assert!(registry.get(&grant().adapter_id).is_none());

    let modified = TemporaryFile::new("json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(profile()).unwrap()).unwrap();
    config["unit"] = json!("unconfirmed-unit");
    std::fs::write(&modified.0, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&modified.0),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    assert!(matches!(
        adapter.poll(&mut observations),
        Err(StdioError::Observation(ObservationError::InvalidPayload(_)))
    ));
    assert!(store.is_empty().unwrap());
    assert!(matches!(
        adapter.poll(&mut observations),
        Err(StdioError::Disconnected)
    ));
}

#[cfg(unix)]
#[test]
fn commit_survives_lost_ack_confirmation_and_reconnect_reuses_it() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let manifest = json!({
        "protocol_version": 1, "request_id": "stdio-1",
        "message": {"type": "manifest", "manifest": {
            "protocol_version": 1, "adapter_id": "demo.sensor", "adapter_version": "1.0.0",
            "capabilities": [{"id": "temperature.observe", "contract": {
                "kind": "observe", "emits": ["arcs.measurement.temperature.v1"]
            }, "required_permissions": ["sensor.read"]}]
        }}
    });
    let observation = json!({
        "protocol_version": 1, "request_id": "stdio-2", "message": {
            "type": "observation", "message_id": "temperature-01:sample-1", "observation": {
                "capability_id": "temperature.observe", "external_subject": "temperature-01",
                "external_reference": "simulated:temperature-01:sample-1",
                "payload": {"value": 25.0, "unit": "Cel"}
            }
        }
    });
    let script = "IFS= read -r request || exit 1\nprintf '%s\\n' \"$1\"\nIFS= read -r request || exit 1\nprintf '%s\\n' \"$2\"\nIFS= read -r request || exit 1\nexit 0";
    let mut command = fixture(script, &manifest.to_string());
    command.arg(observation.to_string());
    let mut registry = AdapterRegistry::new();
    let mut adapter = StdioAdapter::connect(
        &mut command,
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    assert!(matches!(
        adapter.poll(&mut observations),
        Err(StdioError::Wire(WireError::Disconnected))
    ));
    let committed = ObservationLog::new(&store).after(None).unwrap().remove(0);
    assert_eq!(store.len().unwrap(), 1);
    let mut adapter = StdioAdapter::connect(
        &mut sensor(&profile()),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .unwrap();
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    assert_eq!(adapter.poll(&mut observations).unwrap().unwrap(), committed);
    assert_eq!(store.len().unwrap(), 1);
}

#[cfg(unix)]
fn fixture(script: &str, response: &str) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .arg("adapter-fixture")
        .arg(response);
    command
}

#[cfg(unix)]
#[test]
fn connection_rejects_wrong_correlation_version_and_disconnected_process() {
    let schemas = schemas();
    for (version, request_id) in [(WIRE_VERSION, "wrong"), (99, "stdio-1")] {
        let response = json!({"protocol_version": version, "request_id": request_id, "message": {"type": "idle"}});
        let mut registry = AdapterRegistry::new();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let result = StdioAdapter::connect(
            &mut fixture("printf '%s\\n' \"$1\"", &response.to_string()),
            Duration::from_secs(3),
            grant(),
            &mut registry,
            &schemas,
            &store,
        );
        if version == WIRE_VERSION {
            assert!(matches!(result, Err(StdioError::CorrelationMismatch)));
        } else {
            assert!(matches!(result, Err(StdioError::UnsupportedVersion(99))));
        }
        assert!(store.is_empty().unwrap());
        assert!(registry.get(&grant().adapter_id).is_none());
    }
    let mut registry = AdapterRegistry::new();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let result = StdioAdapter::connect(
        &mut fixture("exit 0", ""),
        Duration::from_secs(3),
        grant(),
        &mut registry,
        &schemas,
        &store,
    );
    assert!(matches!(
        result,
        Err(StdioError::Wire(WireError::Disconnected))
    ));
}

#[cfg(unix)]
#[test]
fn stalled_adapter_times_out_and_does_not_register() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    let started = std::time::Instant::now();
    let result = StdioAdapter::connect(
        &mut fixture("exec sleep 10", ""),
        Duration::from_millis(150),
        grant(),
        &mut registry,
        &schemas,
        &store,
    );
    assert!(matches!(result, Err(StdioError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(registry.get(&grant().adapter_id).is_none());
}

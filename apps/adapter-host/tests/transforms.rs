mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::core::{SchemaId, SchemaRegistry, SourceKind, SystemClock, TrustLevel, VersionId};
use arcs::input::{InputMessage, InputService};
use arcs::runtime::{InvocationKind, InvocationService, InvocationSpec, InvocationStatus};
use arcs::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use arcs::transform::TransformError;
use arcs_host::{Host, HostError};
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    config: Value,
    grant: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-transform-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut config: Value =
            serde_json::from_str(include_str!("../config/transform.json")).unwrap();
        config["database"] = json!("db/core.sqlite");
        config["adapters"][0]["program"] = json!(support::adapter_binary("arcs-normalize-number"));
        config["adapters"][0]["grant_file"] = json!("grant.json");
        config["schemas"] = json!([
            root.join("adapters/normalize-number/schemas/raw-number.schema.json"),
            root.join("adapters/normalize-number/schemas/normalized-number.schema.json")
        ]);
        let fixture = Self {
            directory,
            config,
            grant: serde_json::from_str(include_str!("../config/grants/normalize.json")).unwrap(),
        };
        fixture.save();
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn save(&self) {
        std::fs::write(
            self.path("host.json"),
            serde_json::to_vec(&self.config).unwrap(),
        )
        .unwrap();
        std::fs::write(
            self.path("grant.json"),
            serde_json::to_vec(&self.grant).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Result<Host, HostError> {
        Host::open(self.path("host.json"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn capability() -> CapabilityRef {
    CapabilityRef::new("example.normalize", "number.normalize")
}

fn register_source(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    schema: &str,
) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            AdapterManifest {
                protocol_version: 1,
                adapter_id: AdapterId("other.source".into()),
                adapter_version: "1".into(),
                capabilities: vec![CapabilityDescriptor {
                    id: CapabilityId("value.input".into()),
                    contract: CapabilityContract::Input {
                        emits: vec![SchemaId(schema.into())],
                    },
                    required_permissions: vec![],
                }],
            },
            AdapterGrant {
                adapter_id: AdapterId("other.source".into()),
                producer_class: ProducerClass::Adapter,
                enabled_capabilities: vec![CapabilityId("value.input".into())],
                granted_permissions: vec![],
                assigned_trust: TrustLevel::Low,
                ingress_source_kind: Some(SourceKind::Sensor),
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                reasoning_limits: None,
                data_limits: None,
            },
            schemas,
            store,
        )
        .unwrap();
    registry
}

#[test]
fn external_transform_has_provenance_and_persistent_replay_without_world_or_action_changes() {
    let fixture = Fixture::new();
    let mut host = fixture.open().unwrap();
    let result = host.transform_configured("normalize-1").unwrap();
    assert_eq!(result.payload, json!({"value":31.0,"unit":"Cel"}));
    assert_eq!(result.trust.level, TrustLevel::Medium);
    let report = host.report().unwrap();
    assert_eq!(report.stored_artifacts, 6);
    assert_eq!(report.world_revision, 0);
    let input = report.adapters[0].transforms[0].input_version_id.clone();
    assert_eq!(
        result.provenance.as_ref().unwrap().parents,
        vec![input.0.clone()]
    );
    assert!(ArtifactRelations::new(host.store()).outgoing(&result.version_id).unwrap().iter().any(|relation|relation.to==input && relation.kind==relation_kinds::derived_from()));
    assert_eq!(
        host.transform(
            &capability(),
            &input,
            &SchemaId("arcs.number.normalized.v1".into())
        )
        .unwrap(),
        result
    );
    assert_eq!(host.transform_configured("normalize-1").unwrap(), result);
    assert_eq!(host.report().unwrap().stored_artifacts, 6);
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open().unwrap();
    assert_eq!(host.transform_configured("normalize-1").unwrap(), result);
    host.poll_round().unwrap();
    assert_eq!(host.report().unwrap().stored_artifacts, 6);
    assert_eq!(host.report().unwrap().world_revision, 0);
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|entry| !matches!(
                entry.artifact.artifact_type.as_str(),
                "action" | "approval" | "execution_result"
            ))
    );
    host.shutdown().unwrap();
}

#[test]
fn changed_parameters_require_a_new_configured_id() {
    let mut fixture = Fixture::new();
    fixture.open().unwrap().run().unwrap();
    fixture.config["adapters"][0]["transforms"][0]["payload"]["scale"] = json!(0.2);
    fixture.save();
    let mut host = fixture.open().unwrap();
    assert!(matches!(
        host.transform_configured("normalize-1"),
        Err(HostError::Input { .. })
    ));
    host.shutdown().unwrap();
    fixture.config["adapters"][0]["transforms"][0]["id"] = json!("normalize-2");
    fixture.save();
    let report = fixture.open().unwrap().run().unwrap();
    assert_eq!(
        report.adapters[0].transforms[0].payload,
        json!({"value":62.0,"unit":"Cel"})
    );
}

#[test]
fn generic_application_can_transform_existing_low_trust_artifacts() {
    let fixture = Fixture::new();
    let mut host = fixture.open().unwrap();
    let registry = register_source(host.store(), host.schemas(), "arcs.number.raw.v1");
    let input = InputService::new(
        &registry,
        host.schemas(),
        host.store(),
        &mut host.store().durable_id_generator("source").unwrap(),
        &SystemClock,
    )
    .ingest(
        &AdapterId("other.source".into()),
        InputMessage {
            capability_id: CapabilityId("value.input".into()),
            external_subject: Some("measurement-1".into()),
            external_reference: "measurement:1".into(),
            payload: json!({"value":10.0,"scale":2.0,"offset":-1.0,"unit":"m/s"}),
        },
    )
    .unwrap();
    let result = host
        .transform(
            &capability(),
            &input.version_id,
            &SchemaId("arcs.number.normalized.v1".into()),
        )
        .unwrap();
    assert_eq!(result.payload, json!({"value":19.0,"unit":"m/s"}));
    assert_eq!(result.trust.level, TrustLevel::Low);
    assert_eq!(result.provenance.unwrap().parents, vec![input.version_id.0]);
    host.shutdown().unwrap();
}

#[test]
fn static_transform_configuration_errors_do_not_create_storage() {
    let mut fixture = Fixture::new();
    let valid = fixture.config.clone();
    for change in 0..6 {
        fixture.config = valid.clone();
        match change {
            0 => fixture.config["adapters"][0]["transforms"][0]["unexpected"] = json!(true),
            1 => {
                let duplicate = fixture.config["adapters"][0]["transforms"][0].clone();
                fixture.config["adapters"][0]["transforms"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            2 => {
                fixture.config["adapters"][0]["transforms"][0]["payload"]["value"] = json!("wrong")
            }
            3 => fixture.config["adapters"][0]["transforms"][0]["subject"] = json!(""),
            4 => {
                fixture.config["adapters"][0]["transforms"][0]["output_schema_id"] =
                    json!("arcs.unknown.v1")
            }
            _ => {
                fixture.config["adapters"][0]["transforms"][0]["capability_id"] = json!("disabled")
            }
        }
        fixture.save();
        assert!(fixture.open().is_err());
        assert!(!fixture.path("db").exists());
    }
}

#[test]
fn transform_capability_cannot_be_dispatched_as_a_request() {
    let mut fixture = Fixture::new();
    let mut request = fixture.config["adapters"][0]["transforms"][0].clone();
    request["response_schema_id"] = request["output_schema_id"].take();
    request.as_object_mut().unwrap().remove("output_schema_id");
    fixture.config["adapters"][0]["transforms"] = json!([]);
    fixture.config["adapters"][0]["requests"] = json!([request]);
    fixture.save();
    assert!(matches!(fixture.open(), Err(HostError::InvalidConfig(_))));
    assert_eq!(
        SqliteArtifactStore::open(fixture.path("db/core.sqlite"))
            .unwrap()
            .len()
            .unwrap(),
        0
    );
}

#[test]
fn act_capability_cannot_be_dispatched_as_a_transform() {
    let mut fixture = Fixture::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = root.join("apps/adapter-host/config");
    fixture.config =
        serde_json::from_slice(&std::fs::read(base.join("fan.json")).unwrap()).unwrap();
    fixture.config["database"] = json!("db/core.sqlite");
    for schema in fixture.config["schemas"].as_array_mut().unwrap() {
        *schema = json!(base.join(schema.as_str().unwrap()));
    }
    fixture.config["adapters"][0]["program"] = json!(support::adapter_binary("arcs-simulated-fan"));
    fixture.config["adapters"][0]["profile"] =
        json!(root.join("adapters/simulated-fan/config/profile.json"));
    fixture.config["adapters"][0]["args"] = json!([{"path":"db/fan.sqlite"}]);
    fixture.config["adapters"][0]["grant_file"] = json!("grant.json");
    fixture.config["adapters"][0]["requests"] = json!([]);
    fixture.config["adapters"][0]["transforms"] = json!([{"id":"forbidden-act","capability_id":"fan.set","input_schema_id":"arcs.fan_candidate.v1","output_schema_id":"arcs.execution_result.fan.v1","subject":"fan-01","payload":{"device_id":"fan-01","enabled":true}}]);
    fixture.grant = serde_json::from_str(include_str!("../config/grants/fan.json")).unwrap();
    fixture.save();
    assert!(matches!(fixture.open(), Err(HostError::InvalidConfig(_))));
    assert_eq!(
        SqliteArtifactStore::open(fixture.path("db/core.sqlite"))
            .unwrap()
            .len()
            .unwrap(),
        0
    );
}

#[test]
fn rejected_normalization_does_not_persist_a_successful_result() {
    let mut fixture = Fixture::new();
    fixture.config["adapters"][0]["transforms"][0]["payload"] =
        json!({"value":1e308,"scale":1e308,"offset":0.0,"unit":"V"});
    fixture.save();
    let mut host = fixture.open().unwrap();
    assert!(matches!(
        host.transform_configured("normalize-1"),
        Err(HostError::Transform {
            source: TransformError::AdapterCall(_),
            ..
        })
    ));
    let count = host.store().len().unwrap();
    drop(host);
    let mut host = fixture.open().unwrap();
    assert!(matches!(
        host.transform_configured("normalize-1"),
        Err(HostError::Transform {
            source: TransformError::Invocation(_),
            ..
        })
    ));
    assert_eq!(host.store().len().unwrap(), count);
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|entry| entry.artifact.schema_id.0 != "arcs.number.normalized.v1")
    );
    host.shutdown().unwrap();
}

#[test]
fn legacy_v1_invocation_schema_and_states_survive_adding_transform_support() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.path("db")).unwrap();
    let mut old_schemas = SchemaRegistry::new();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/arcs-core/schemas/v1");
    for file in std::fs::read_dir(path).unwrap() {
        old_schemas
            .register_json(&std::fs::read_to_string(file.unwrap().path()).unwrap())
            .unwrap();
    }
    let store = SqliteArtifactStore::open(fixture.path("db/core.sqlite")).unwrap();
    let _registry = register_source(&store, &old_schemas, "arcs.input.v1");
    let legacy = InvocationService::new(&store, &old_schemas, &SystemClock)
        .prepare(InvocationSpec {
            invocation_id: "request:legacy".into(),
            kind: InvocationKind::Request,
            capability: "legacy/read".into(),
            input_version: VersionId("legacy-input".into()),
            input_fingerprint: "legacy".into(),
        })
        .unwrap();
    assert_eq!(legacy.status, InvocationStatus::Prepared);
    drop(store);
    let mut host = fixture.open().unwrap();
    host.transform_configured("normalize-1").unwrap();
    let recovered = InvocationService::new(host.store(), host.schemas(), &SystemClock)
        .lookup("request:legacy")
        .unwrap()
        .unwrap();
    assert_eq!(recovered, legacy);
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .any(|entry| entry.artifact.schema_id.0 == "arcs.invocation.v2")
    );
    host.shutdown().unwrap();
}

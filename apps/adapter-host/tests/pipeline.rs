#[path = "../examples/observe-report/pipeline.rs"]
mod pipeline;
mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::adapters::CapabilityRef;
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SourceClass, SystemClock, TrustLevel};
use arcs::store::{ArtifactRelations, relation_kinds};
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs::world::observation::ObservationLog;
use arcs_host::Host;
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    sensor: Value,
    pipeline: Value,
}

impl Fixture {
    fn new(schema: &str, unit: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-pipeline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let host = json!({
            "config_version":1,"database":"db/core.sqlite","node_id":"pipeline-test",
            "schemas":[root.join("adapters/simulated-sensor/schemas/temperature.schema.json"),root.join("adapters/normalize-number/schemas/normalized-number.schema.json"),root.join("adapters/file-output/schemas/report.schema.json"),root.join("adapters/file-output/schemas/receipt.schema.json")],
            "adapters":[
                {"program":support::adapter_binary("arcs-simulated-sensor"),"profile":"sensor.json","grant_file":"sensor-grant.json"},
                {"program":support::adapter_binary("arcs-json-report"),"profile":"formatter.json","grant_file":"formatter-grant.json"},
                {"program":support::adapter_binary("arcs-file-output"),"profile":"sink.json","args":[{"path":"db/sink.sqlite"}],"grant_file":"sink-grant.json"}
            ]
        });
        let mut source_grant: Value =
            serde_json::from_str(include_str!("../config/grants/sensor.json")).unwrap();
        source_grant["assigned_trust"] = json!("low");
        let fixture = Self {
            directory,
            sensor: json!({"adapter_id":"demo.sensor","device_id":"source-1","capability_id":"temperature.observe","schema_id":schema,"field":"raw","scale":0.1,"offset":0.0,"unit":unit,
                "samples":[{"id":"sample-1","raw":{"raw":250}},{"id":"sample-2","raw":{"raw":260}}]}),
            pipeline: json!({"config_version":2,"pipeline_id":"test-pipeline","source":{"adapter_id":"demo.sensor","capability_id":"temperature.observe"},"input_schema_id":schema,
                "steps":[{"id":"render","capability":{"adapter_id":"example.report","capability_id":"json.report"},"output_schema_id":"arcs.report.v1"}],
                "output":{"adapter_id":"example.file-output","capability_id":"report.write"},"receipt_schema_id":"arcs.delivery_receipt.file.v1","max_observations":10}),
        };
        fixture.write("host.json", &host);
        fixture.write("sensor-grant.json", &source_grant);
        fixture.write("formatter.json", &json!({"adapter_id":"example.report","accepts":["arcs.measurement.temperature.v1","arcs.number.normalized.v1"]}));
        fixture.write(
            "formatter-grant.json",
            &serde_json::from_str::<Value>(include_str!("../config/grants/report.json")).unwrap(),
        );
        fixture.write(
            "sink.json",
            &json!({"adapter_id":"example.file-output","directory":"reports"}),
        );
        fixture.write(
            "sink-grant.json",
            &serde_json::from_str::<Value>(include_str!("../config/grants/output.json")).unwrap(),
        );
        fixture.save_sensor();
        fixture
    }
    fn write(&self, name: &str, value: &Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
    fn save_sensor(&self) {
        self.write("sensor.json", &self.sensor);
    }
    fn open(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn config(&self) -> pipeline::PipelineConfig {
        serde_json::from_value(self.pipeline.clone()).unwrap()
    }
    fn files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.directory.join("reports"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect()
    }

    fn observation_peer(&self, host: &mut Value, schema: &str, payload: Value) {
        host["adapters"][0] = json!({
            "program":"/usr/bin/python3",
            "args":[{"path":"routing-observer.py"},{"path":"routing-observer.json"}],
            "grant_file":"sensor-grant.json"
        });
        self.write(
            "routing-observer.json",
            &json!({"schema":schema,"payload":payload}),
        );
        std::fs::write(
        self.directory.join("routing-observer.py"),
        r#"import json, sys
profile = json.load(open(sys.argv[1]))
sent = False
for line in sys.stdin:
    packet = json.loads(line)
    kind = packet['message']['type']
    if kind == 'hello':
        reply = {'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'demo.sensor','adapter_version':'1','capabilities':[{'id':'temperature.observe','contract':{'kind':'observe','emits':[profile['schema']]},'required_permissions':['sensor.read']}]}}
    elif kind == 'poll':
        reply = {'type':'idle'} if sent else {'type':'observation','message_id':'reading-1','observation':{'capability_id':'temperature.observe','external_subject':'source-1','external_reference':'reading-1','payload':profile['payload']}}
        sent = True
    elif kind == 'ack':
        reply = {'type':'acknowledged','message_id':packet['message']['message_id']}
    elif kind == 'shutdown':
        reply = {'type':'bye'}
    else:
        raise RuntimeError(kind)
    print(json.dumps({'protocol_version':1,'request_id':packet['request_id'],'message':reply}),flush=True)
    if kind == 'shutdown':
        break
"#,
    )
    .unwrap();
    }

    fn two_steps(&mut self, maximum: usize) {
        let mut host: Value =
            serde_json::from_slice(&std::fs::read(self.directory.join("host.json")).unwrap())
                .unwrap();
        host["adapters"].as_array_mut().unwrap().push(json!({
            "program":support::adapter_binary("arcs-json-report"),"profile":"compactor.json","grant_file":"compactor-grant.json"
        }));
        self.write("host.json", &host);
        self.write(
            "compactor.json",
            &json!({"adapter_id":"example.compactor","accepts":["arcs.report.v1"]}),
        );
        self.compactor_limit(maximum);
        self.pipeline["steps"].as_array_mut().unwrap().push(json!({
            "id":"compact","capability":{"adapter_id":"example.compactor","capability_id":"report.compact"},"output_schema_id":"arcs.report.v1"
        }));
    }

    fn compactor_limit(&self, maximum: usize) {
        let mut grant: Value =
            serde_json::from_str(include_str!("../config/grants/report.json")).unwrap();
        grant["adapter_id"] = json!("example.compactor");
        grant["enabled_capabilities"] = json!(["report.compact"]);
        grant["max_payload_bytes"] = json!(maximum);
        self.write("compactor-grant.json", &grant);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn three_processes_preserve_evidence_trust_and_delivery_identity_across_restart() {
    let fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    let mut host = fixture.open();
    let first = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(first.deliveries.len(), 2);
    assert_eq!(fixture.files().len(), 2);
    assert_eq!(first.host.world_revision, 2);
    for delivery in &first.deliveries {
        assert_eq!(delivery.transformed_trust.level, TrustLevel::Low);
        assert_eq!(
            delivery.transformed_trust.source_class,
            SourceClass::External
        );
        let input = host
            .store()
            .get(&delivery.input_version_id)
            .unwrap()
            .unwrap();
        let output = host
            .store()
            .get(&delivery.transformed_version_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            output.provenance.as_ref().unwrap().parents,
            vec![input.version_id.0.clone()]
        );
        let relations = ArtifactRelations::new(host.store())
            .outgoing(&output.version_id)
            .unwrap();
        assert!(
            relations
                .iter()
                .any(|edge| edge.to == input.version_id
                    && edge.kind == relation_kinds::derived_from())
        );
        let filename = delivery.receipt["file_name"].as_str().unwrap();
        let envelope: Value = serde_json::from_slice(
            &std::fs::read(fixture.directory.join("reports").join(filename)).unwrap(),
        )
        .unwrap();
        assert_eq!(envelope["artifact_version_id"], json!(output.version_id));
        assert_eq!(
            envelope["request_version_id"],
            json!(delivery.request_version_id)
        );
        assert_eq!(
            envelope["approval_version_id"],
            json!(delivery.approval_version_id)
        );
        let body: Value =
            serde_json::from_str(envelope["payload"]["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["version_id"], json!(input.version_id));
        assert_eq!(body["payload"], input.payload);
    }
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    let second = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert!(second.deliveries.is_empty());
    assert_eq!(second.completed_cursor, first.completed_cursor);
    assert_eq!(second.checkpoint_version_id, first.checkpoint_version_id);
    assert_eq!(second.host.stored_artifacts, first.host.stored_artifacts);
    assert_eq!(second.host.world_revision, 2);
    assert_eq!(fixture.files().len(), 2);
    host.shutdown().unwrap();
}

#[test]
fn another_unit_and_schema_use_the_same_chain_and_only_selected_source_is_exported() {
    let fixture = Fixture::new("arcs.number.normalized.v1", "m/s");
    let mut host_config: Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("host.json")).unwrap())
            .unwrap();
    let mut decoy = fixture.sensor.clone();
    decoy["adapter_id"] = json!("decoy.sensor");
    decoy["samples"][0]["raw"]["raw"] = json!(999);
    fixture.write("decoy.json", &decoy);
    let mut grant: Value =
        serde_json::from_str(include_str!("../config/grants/sensor.json")).unwrap();
    grant["adapter_id"] = json!("decoy.sensor");
    fixture.write("decoy-grant.json", &grant);
    let mut extra = host_config["adapters"][0].clone();
    extra["profile"] = json!("decoy.json");
    extra["grant_file"] = json!("decoy-grant.json");
    host_config["adapters"].as_array_mut().unwrap().push(extra);
    fixture.write("host.json", &host_config);
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(report.deliveries.len(), 2);
    assert_eq!(report.host.world_revision, 4);
    for delivery in report.deliveries {
        let input = host
            .store()
            .get(&delivery.input_version_id)
            .unwrap()
            .unwrap();
        assert_eq!(input.created_by.id, "demo.sensor");
        assert_eq!(input.payload["unit"], "m/s");
    }
    host.shutdown().unwrap();
}

#[test]
fn incompatible_roles_and_schemas_fail_before_polling_and_publication() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    let original = fixture.pipeline.clone();
    for (field, replacement) in [
        (
            "source",
            json!({"adapter_id":"example.report","capability_id":"json.report"}),
        ),
        (
            "steps",
            json!([{"id":"bad","capability":{"adapter_id":"demo.sensor","capability_id":"temperature.observe"},"output_schema_id":"arcs.report.v1"}]),
        ),
        (
            "steps",
            json!([{"id":"render","capability":{"adapter_id":"example.report","capability_id":"json.report"},"output_schema_id":"arcs.number.normalized.v1"}]),
        ),
        ("receipt_schema_id", json!("arcs.report.v1")),
    ] {
        fixture.pipeline = original.clone();
        fixture.pipeline[field] = replacement;
        let mut host = fixture.open();
        assert!(pipeline::run(&mut host, &fixture.config()).is_err());
        assert_eq!(host.report().unwrap().stored_artifacts, 0);
        assert!(fixture.files().is_empty());
        host.shutdown().unwrap();
    }
}

#[test]
fn per_run_budget_advances_the_cursor_without_reprocessing_history() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.pipeline["max_observations"] = json!(1);
    let mut host = fixture.open();
    let first = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(first.deliveries.len(), 1);
    assert_eq!(first.remaining_observations, 1);
    assert_eq!(host.report().unwrap().world_revision, 2);
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
    drop(host);
    fixture.pipeline["max_observations"] = json!(2);
    let mut host = fixture.open();
    let second = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(second.deliveries.len(), 1);
    assert_eq!(second.remaining_observations, 0);
    assert!(second.completed_cursor > first.completed_cursor);
    assert_ne!(
        second.deliveries[0].input_version_id,
        first.deliveries[0].input_version_id
    );
    assert_eq!(fixture.files().len(), 2);
    host.shutdown().unwrap();
}

#[test]
fn revoked_transform_cannot_use_cached_results() {
    let fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    let mut host = fixture.open();
    let first = pipeline::run(&mut host, &fixture.config()).unwrap();
    host.shutdown().unwrap();
    drop(host);
    let mut grant: Value =
        serde_json::from_str(include_str!("../config/grants/report.json")).unwrap();
    grant["granted_permissions"] = json!([]);
    fixture.write("formatter-grant.json", &grant);
    let error = Host::open(fixture.directory.join("host.json"))
        .err()
        .unwrap();
    assert!(error.to_string().contains("MissingPermission"));
    assert_eq!(
        arcs::store::SqliteArtifactStore::open(fixture.directory.join("db/core.sqlite"))
            .unwrap()
            .len()
            .unwrap(),
        first.host.stored_artifacts
    );
    assert_eq!(fixture.files().len(), 2);
}

#[test]
fn restart_with_a_prepared_output_resumes_the_same_transform_and_request() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.sensor["samples"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    fixture.save_sensor();
    let mut host = fixture.open();
    host.poll_round().unwrap();
    let input = ObservationLog::new(host.store())
        .after(None)
        .unwrap()
        .remove(0)
        .into_artifact();
    let transformed = host
        .transform(
            &CapabilityRef::new("example.report", "json.report"),
            &input.version_id,
            &SchemaId("arcs.report.v1".into()),
        )
        .unwrap();
    let request = host
        .prepare_output(
            &CapabilityRef::new("example.file-output", "report.write"),
            &transformed.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(report.deliveries.len(), 1);
    assert_eq!(report.deliveries[0].input_version_id, input.version_id);
    assert_eq!(
        report.deliveries[0].transformed_version_id,
        transformed.version_id
    );
    assert_eq!(report.deliveries[0].request_version_id, request.version_id);
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

struct Check;
impl Verifier for Check {
    fn verify(&self, _: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: VerificationVerdict::Pass,
            findings: vec![],
        })
    }
}

#[test]
fn restart_after_transform_preserves_a_rejection_instead_of_auto_approving_it() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.sensor["samples"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    fixture.save_sensor();
    let mut host = fixture.open();
    host.poll_round().unwrap();
    let input = ObservationLog::new(host.store())
        .after(None)
        .unwrap()
        .remove(0)
        .into_artifact();
    let transformed = host
        .transform(
            &CapabilityRef::new("example.report", "json.report"),
            &input.version_id,
            &SchemaId("arcs.report.v1".into()),
        )
        .unwrap();
    let request = host
        .prepare_output(
            &CapabilityRef::new("example.file-output", "report.write"),
            &transformed.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    let mut ids = host.store().durable_id_generator("test-policy").unwrap();
    let verification =
        VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
            .verify(&request.version_id, &Check)
            .unwrap();
    let rejection = ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .decide(
            &request.version_id,
            &verification.version_id,
            ApprovalDecision::Rejected,
            Actor {
                actor_type: ActorType::System,
                id: "demo.pipeline.configured-export.v1".into(),
            },
            "explicit denial".into(),
        )
        .unwrap();
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    assert!(pipeline::run(&mut host, &fixture.config()).is_err());
    assert!(fixture.files().is_empty());
    assert!(host.report().unwrap().stored_artifacts > count);
    assert_eq!(
        host.store()
            .find_by_stream_key(&format!("approval:{}", request.version_id.0))
            .unwrap()
            .unwrap()
            .version_id,
        rejection.version_id
    );
    host.shutdown().unwrap();
}

fn checkpoint(host: &Host) -> Value {
    host.store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|stored| stored.artifact)
        .rfind(|artifact| {
            artifact.schema_id.0 == "arcs.input.host_pipeline.v1"
                && artifact.payload["kind"] == "checkpoint"
        })
        .unwrap()
        .payload["data"]["progress"]
        .clone()
}

#[test]
fn two_transforms_across_four_processes_keep_all_provenance_and_trust() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.two_steps(4096);
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(report.deliveries.len(), 2);
    for delivery in report.deliveries {
        assert_eq!(delivery.steps.len(), 2);
        let mut previous = delivery.input_version_id;
        for step in &delivery.steps {
            assert_eq!(step.input_version_id, previous);
            assert_eq!(step.trust.level, TrustLevel::Low);
            let artifact = host.store().get(&step.version_id).unwrap().unwrap();
            assert_eq!(
                artifact.provenance.as_ref().unwrap().parents,
                vec![previous.0.clone()]
            );
            previous = step.version_id.clone();
        }
        let first = host
            .store()
            .get(&delivery.steps[0].version_id)
            .unwrap()
            .unwrap();
        let last = host
            .store()
            .get(&delivery.steps[1].version_id)
            .unwrap()
            .unwrap();
        assert!(
            last.payload["body"].as_str().unwrap().len()
                < first.payload["body"].as_str().unwrap().len()
        );
        assert_eq!(
            serde_json::from_str::<Value>(first.payload["body"].as_str().unwrap()).unwrap(),
            serde_json::from_str::<Value>(last.payload["body"].as_str().unwrap()).unwrap()
        );
    }
    assert!(checkpoint(&host)["active"].is_null());
    assert_eq!(fixture.files().len(), 2);
    host.shutdown().unwrap();
}

#[test]
fn second_step_failure_persists_the_first_result_and_resumes_after_limit_fix() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.sensor["samples"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    fixture.save_sensor();
    fixture.two_steps(1);
    let mut host = fixture.open();
    assert!(pipeline::run(&mut host, &fixture.config()).is_err());
    let failed = checkpoint(&host);
    assert_eq!(failed["cursor"], 0);
    assert_eq!(
        failed["active"]["step_versions"].as_array().unwrap().len(),
        1
    );
    assert!(failed["active"]["request_version_id"].is_null());
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .starts_with("transform:compact:")
    );
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
    drop(host);
    fixture.compactor_limit(4096);
    let mut host = fixture.open();
    let resumed = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(resumed.deliveries.len(), 1);
    assert_eq!(
        json!(resumed.deliveries[0].steps[0].version_id),
        failed["active"]["step_versions"][0]
    );
    assert_eq!(resumed.deliveries[0].steps.len(), 2);
    assert!(checkpoint(&host)["error"].is_null());
    assert!(checkpoint(&host)["active"].is_null());
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn discovered_two_hop_route_replays_the_same_result_after_restart() {
    let raw = "arcs.number.raw.v1";
    let normalized = "arcs.number.normalized.v1";
    let mut fixture = Fixture::new(raw, "Cel");
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("host.json")).unwrap())
            .unwrap();
    config["schemas"]
        .as_array_mut()
        .unwrap()
        .push(json!(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../adapters/normalize-number/schemas/raw-number.schema.json"
        )));
    fixture.observation_peer(
        &mut config,
        raw,
        json!({"value":250,"scale":0.1,"offset":0,"unit":"Cel"}),
    );
    config["adapters"].as_array_mut().unwrap().push(json!({
        "program":support::adapter_binary("arcs-normalize-number"),
        "grant_file":"normalize-grant.json"
    }));
    fixture.write("host.json", &config);
    fixture.write(
        "normalize-grant.json",
        &serde_json::from_str(include_str!("../config/grants/normalize.json")).unwrap(),
    );
    fixture.write(
        "formatter.json",
        &json!({"adapter_id":"example.report","accepts":[normalized]}),
    );
    let grant: Value = serde_json::from_str(include_str!("../config/grants/report.json")).unwrap();
    let mut limited_grant = grant.clone();
    limited_grant["max_payload_bytes"] = json!(1);
    fixture.write("formatter-grant.json", &limited_grant);
    fixture.pipeline["transform_routing"] = json!({
        "output_schema_id":"arcs.report.v1",
        "allowed_capabilities":[
            {"adapter_id":"example.report","capability_id":"json.report"},
            {"adapter_id":"example.normalize","capability_id":"number.normalize"}
        ],
        "max_steps":1
    });
    let mut host = fixture.open();
    let error = pipeline::run(&mut host, &fixture.config()).unwrap_err();
    assert!(error.to_string().contains("mutually exclusive"));
    fixture.pipeline.as_object_mut().unwrap().remove("steps");
    let error = pipeline::run(&mut host, &fixture.config()).unwrap_err();
    assert!(error.to_string().contains("no authorized transform path"));
    assert_eq!(host.report().unwrap().stored_artifacts, 0);

    fixture.pipeline["transform_routing"]["max_steps"] = json!(2);
    let error = pipeline::run(&mut host, &fixture.config()).unwrap_err();
    let failed = checkpoint(&host);
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .starts_with("transform:transform-2:"),
        "{error:?}, {failed}",
    );
    let first = failed["active"]["step_versions"][0].clone();
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
    drop(host);

    fixture.write("formatter-grant.json", &grant);
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(report.deliveries.len(), 1);
    let delivery = &report.deliveries[0];
    assert_eq!(delivery.steps.len(), 2);
    assert_eq!(delivery.steps[0].schema_id.0, normalized);
    assert_eq!(json!(delivery.steps[0].version_id), first);
    assert_eq!(delivery.steps[1].schema_id.0, "arcs.report.v1");
    assert_eq!(
        delivery.steps[1].input_version_id,
        delivery.steps[0].version_id
    );
    assert_eq!(delivery.transformed_trust.level, TrustLevel::Low);
    assert_eq!(fixture.files().len(), 1);
    assert!(
        pipeline::run(&mut host, &fixture.config())
            .unwrap()
            .deliveries
            .is_empty()
    );
    assert_eq!(fixture.files().len(), 1);

    fixture.pipeline["transform_routing"]["max_steps"] = json!(3);
    assert!(pipeline::run(&mut host, &fixture.config()).is_err());
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn routed_transform_only_receives_projected_data_and_reuses_its_version() {
    let raw = "arcs.number.raw.v1";
    let selected = "arcs.projection.reading.v1";
    let mut fixture = Fixture::new(raw, "Cel");
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("host.json")).unwrap())
            .unwrap();
    config["schemas"].as_array_mut().unwrap().extend([
        json!(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../adapters/normalize-number/schemas/raw-number.schema.json")
        ),
        json!("selected-reading.schema.json"),
    ]);
    fixture.write(
        "selected-reading.schema.json",
        &json!({
            "$id":selected, "type":"object","required":["value","unit"],
            "properties":{"value":{"type":"number"},"unit":{"type":"string"}},
            "additionalProperties":false
        }),
    );
    fixture.observation_peer(
        &mut config,
        raw,
        json!({"value":25,"scale":17,"offset":23,"unit":"Cel"}),
    );
    fixture.write("host.json", &config);
    fixture.write(
        "formatter.json",
        &json!({"adapter_id":"example.report","accepts":[selected]}),
    );
    let grant: Value = serde_json::from_str(include_str!("../config/grants/report.json")).unwrap();
    let mut limited_grant = grant.clone();
    limited_grant["max_payload_bytes"] = json!(1);
    fixture.write("formatter-grant.json", &limited_grant);
    fixture.pipeline.as_object_mut().unwrap().remove("steps");
    fixture.pipeline["transform_routing"] = json!({
        "output_schema_id":"arcs.report.v1",
        "allowed_capabilities":[{"adapter_id":"example.report","capability_id":"json.report"}],
        "max_steps":1
    });
    fixture.pipeline["input_projection"] = json!({
        "schema_id":"arcs.action.v1","payload_fields":["value","unit"]
    });
    let mut host = fixture.open();
    let error = pipeline::run(&mut host, &fixture.config()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("dedicated arcs.projection schema")
    );
    assert_eq!(host.report().unwrap().stored_artifacts, 0);

    fixture.pipeline["input_projection"]["schema_id"] = json!(selected);
    let error = pipeline::run(&mut host, &fixture.config()).unwrap_err();
    assert!(format!("{error:?}").contains("InputTooLarge"), "{error:?}");
    let projections = |host: &Host| {
        host.store()
            .committed_after(0)
            .unwrap()
            .into_iter()
            .map(|recorded| recorded.artifact)
            .filter(|artifact| artifact.schema_id.0 == selected)
            .collect::<Vec<_>>()
    };
    let saved = projections(&host);
    assert_eq!(saved.len(), 1);
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
    drop(host);

    fixture.write("formatter-grant.json", &grant);
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    let delivery = &report.deliveries[0];
    assert_eq!(
        delivery.projection_version_id.as_ref(),
        Some(&saved[0].version_id)
    );
    assert_eq!(delivery.steps[0].input_version_id, saved[0].version_id);
    assert_eq!(projections(&host), saved);
    let original = host
        .store()
        .get(&delivery.input_version_id)
        .unwrap()
        .unwrap();
    assert_eq!(original.payload["scale"], 17);
    assert_eq!(
        saved[0].provenance.as_ref().unwrap().parents,
        [original.version_id.0]
    );
    assert_eq!(delivery.transformed_trust.level, TrustLevel::Low);
    let rendered = host
        .store()
        .get(&delivery.transformed_version_id)
        .unwrap()
        .unwrap();
    let body: Value = serde_json::from_str(rendered.payload["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["payload"], json!({"value":25,"unit":"Cel"}));
    assert_eq!(fixture.files().len(), 1);
    assert!(
        pipeline::run(&mut host, &fixture.config())
            .unwrap()
            .deliveries
            .is_empty()
    );
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn transform_workers_share_capacity_and_recover_a_lost_response() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.sensor["samples"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"sample-3","raw":{"raw":270}}));
    fixture.save_sensor();
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("host.json")).unwrap())
            .unwrap();
    config["resource_pools"] = json!({"slot":1});
    config["adapters"][1] = json!({
        "program":"/usr/bin/python3","args":[{"path":"gated-transform.py"}],
        "grant_file":"formatter-grant.json","reasoning_resources":{"slot":1},"timeout_ms":15000
    });
    let mut grant: Value =
        serde_json::from_str(include_str!("../config/grants/report.json")).unwrap();
    grant["enabled_capabilities"] = json!(["json.report"]);
    fixture.write("formatter-grant.json", &grant);
    std::fs::write(
        fixture.directory.join("gated-transform.py"),
        r#"import json, pathlib, sys, time
root = pathlib.Path(__file__).parent
for line in sys.stdin:
    packet = json.loads(line)
    msg = packet['message']
    kind = msg['type']
    if kind == 'hello':
        reply = {'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'example.report','adapter_version':'1','capabilities':[{'id':'json.report','contract':{'kind':'transform','accepts':['arcs.measurement.temperature.v1'],'emits':['arcs.report.v1']},'required_permissions':['data.transform']}]}}
    elif kind == 'transform':
        inv = msg['invocation']
        with (root / 'started').open('a') as started:
            started.write(inv['invocation_id'] + '\n')
        while not (root / 'release').exists():
            time.sleep(.003)
        reply = {'type':'transform_result','response':{'invocation_id':inv['invocation_id'],'external_reference':'transformed','payload':{'title':'Observation','body':json.dumps(inv['input_payload'])}}}
    elif kind == 'shutdown':
        reply = {'type':'bye'}
    else:
        raise RuntimeError(kind)
    print(json.dumps({'protocol_version':1,'request_id':packet['request_id'],'message':reply}),flush=True)
    if kind == 'shutdown':
        break
"#,
    )
    .unwrap();
    for (id, claim) in [
        ("example.shared", json!({"slot":1})),
        ("example.free", json!({})),
    ] {
        grant["adapter_id"] = json!(id);
        fixture.write(&format!("{id}-grant.json"), &grant);
        fixture.write(
            &format!("{id}.json"),
            &json!({
                "adapter_id":id,"accepts":["arcs.measurement.temperature.v1"]
            }),
        );
        config["adapters"].as_array_mut().unwrap().push(json!({
            "program":support::adapter_binary("arcs-json-report"),
            "profile":format!("{id}.json"),"grant_file":format!("{id}-grant.json"),
            "reasoning_resources":claim
        }));
    }
    fixture.write("host.json", &config);
    let mut host = fixture.open();
    host.poll_round().unwrap();
    let inputs = ObservationLog::new(host.store()).after(None).unwrap();
    let first = inputs[0].artifact().version_id.clone();
    let second = inputs[1].artifact().version_id.clone();
    let third = inputs[2].artifact().version_id.clone();
    let gated = CapabilityRef::new("example.report", "json.report");
    let shared = CapabilityRef::new("example.shared", "json.report");
    let free = CapabilityRef::new("example.free", "json.report");
    let output = SchemaId("arcs.report.v1".into());
    let started = || {
        std::fs::read_to_string(fixture.directory.join("started"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let until_started = |count| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while started().len() < count {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    };
    let collect = |host: &mut Host, capability: &CapabilityRef, input: &arcs::core::VersionId| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(result) = host.poll_transform(capability, input, &output, 2).unwrap() {
                break result;
            }
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    };
    assert!(host.poll_transform(&gated, &first, &output, 0).is_err());
    assert!(
        host.poll_transform(&gated, &first, &output, 2)
            .unwrap()
            .is_none()
    );
    until_started(1);
    assert!(
        host.poll_transform(&gated, &first, &output, 2)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        host.transform(&gated, &second, &output),
        Err(arcs_host::HostError::AdapterBusy(_))
    ));
    assert!(
        host.poll_transform(&shared, &first, &output, 2)
            .unwrap()
            .is_none()
    );
    assert!(
        host.poll_transform(&free, &first, &output, 1)
            .unwrap()
            .is_none()
    );
    assert_eq!(host.transform_workers().len(), 1);
    assert_eq!(host.report().unwrap().resource_pools[0].used, 1);
    assert_eq!(
        collect(&mut host, &free, &first).created_by.id,
        "example.free"
    );
    assert_eq!(started().len(), 1);
    assert_eq!(host.transform_workers().len(), 1);

    std::fs::write(fixture.directory.join("release"), "").unwrap();
    let completed = collect(&mut host, &gated, &first);
    assert_eq!(host.report().unwrap().resource_pools[0].used, 0);
    assert_eq!(
        collect(&mut host, &shared, &first).created_by.id,
        "example.shared"
    );
    std::fs::remove_file(fixture.directory.join("release")).unwrap();

    assert!(
        host.poll_transform(&gated, &second, &output, 2)
            .unwrap()
            .is_none()
    );
    until_started(2);
    let interrupted = host.transform_workers().remove(0).invocation_id;
    assert_eq!(
        host.poll_transform(&gated, &first, &output, 1).unwrap(),
        Some(completed)
    );
    host.shutdown().unwrap();
    drop(host);

    std::fs::write(fixture.directory.join("release"), "").unwrap();
    let mut host = fixture.open();
    let resumed = collect(&mut host, &gated, &second);
    assert_eq!(resumed.stream_key, format!("transform:{interrupted}"));
    let calls = started();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1], calls[2]);
    assert_eq!(collect(&mut host, &gated, &second), resumed);
    assert_eq!(started().len(), 3);
    assert!(host.transform_workers().is_empty());
    host.shutdown().unwrap();

    drop(host);

    // A real timeout retires the worker and releases the shared pool.
    std::fs::remove_file(fixture.directory.join("release")).unwrap();
    config["adapters"][1]["timeout_ms"] = json!(1000);
    fixture.write("host.json", &config);
    let mut host = fixture.open();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match host.poll_transform(&gated, &third, &output, 2) {
            Ok(None) => {
                assert!(std::time::Instant::now() < end);
                std::thread::sleep(std::time::Duration::from_millis(3));
            }
            Err(arcs_host::HostError::Transform {
                source:
                    arcs::transform::TransformError::AdapterCall(
                        arcs::adapters::AdapterCallError::Timeout,
                    ),
                ..
            }) => break,
            result => panic!("unexpected timeout result: {result:?}"),
        }
    }
    assert!(host.transform_workers().is_empty());
    assert_eq!(host.report().unwrap().resource_pools[0].used, 0);
    host.shutdown().unwrap();
}

#[test]
fn compatible_input_is_delivered_with_approval_and_no_transform() {
    let schema = "arcs.report.v1";
    let mut fixture = Fixture::new(schema, "Cel");
    let mut config: Value =
        serde_json::from_slice(&std::fs::read(fixture.directory.join("host.json")).unwrap())
            .unwrap();
    fixture.observation_peer(
        &mut config,
        schema,
        json!({"title":"Status","body":"Observed"}),
    );
    fixture.write("host.json", &config);
    fixture.pipeline.as_object_mut().unwrap().remove("steps");
    fixture.pipeline["transform_routing"] = json!({
        "output_schema_id":schema, "allowed_capabilities":[], "max_steps":1
    });
    let mut host = fixture.open();
    let report = pipeline::run(&mut host, &fixture.config()).unwrap();
    let [delivery] = report.deliveries.as_slice() else {
        panic!("expected the original observation to be delivered once");
    };
    assert!(delivery.steps.is_empty());
    assert_eq!(delivery.input_version_id, delivery.transformed_version_id);
    let approval = host
        .store()
        .get(&delivery.approval_version_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        approval.payload["target_version"],
        delivery.request_version_id.0
    );
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn later_schema_errors_and_duplicate_step_ids_fail_before_any_ingest() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.two_steps(4096);
    fixture.pipeline["steps"][1]["output_schema_id"] = json!("arcs.number.normalized.v1");
    let mut host = fixture.open();
    assert!(pipeline::run(&mut host, &fixture.config()).is_err());
    assert_eq!(host.report().unwrap().stored_artifacts, 0);
    fixture.pipeline["steps"][1]["output_schema_id"] = json!("arcs.report.v1");
    fixture.pipeline["steps"][1]["id"] = json!("render");
    assert!(pipeline::run(&mut host, &fixture.config()).is_err());
    assert_eq!(host.report().unwrap().stored_artifacts, 0);
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
}

#[test]
fn bound_pipeline_id_rejects_topology_and_manifest_changes() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    let mut host = fixture.open();
    let first = pipeline::run(&mut host, &fixture.config()).unwrap();
    fixture.pipeline["steps"][0]["id"] = json!("changed-render");
    assert!(
        pipeline::run(&mut host, &fixture.config())
            .unwrap_err()
            .to_string()
            .contains("IdentityConflict")
    );
    assert_eq!(
        host.report().unwrap().stored_artifacts,
        first.host.stored_artifacts
    );
    host.shutdown().unwrap();
    drop(host);
    fixture.pipeline["steps"][0]["id"] = json!("render");
    fixture.write(
        "formatter.json",
        &json!({"adapter_id":"example.report","accepts":["arcs.measurement.temperature.v1"]}),
    );
    let mut host = fixture.open();
    assert!(
        pipeline::run(&mut host, &fixture.config())
            .unwrap_err()
            .to_string()
            .contains("IdentityConflict")
    );
    assert_eq!(fixture.files().len(), 2);
    assert_eq!(
        host.report().unwrap().stored_artifacts,
        first.host.stored_artifacts
    );
    host.shutdown().unwrap();
}

#[test]
fn newly_arriving_observation_is_processed_after_the_saved_cursor() {
    let mut fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    fixture.two_steps(4096);
    let mut host = fixture.open();
    let first = pipeline::run(&mut host, &fixture.config()).unwrap();
    host.shutdown().unwrap();
    drop(host);
    fixture.sensor["samples"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"sample-3","raw":{"raw":270}}));
    fixture.save_sensor();
    let mut host = fixture.open();
    let second = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(second.deliveries.len(), 1);
    assert_eq!(second.host.world_revision, 3);
    assert!(second.completed_cursor > first.completed_cursor);
    assert!(
        first
            .deliveries
            .iter()
            .all(|delivery| delivery.input_version_id != second.deliveries[0].input_version_id)
    );
    assert_eq!(fixture.files().len(), 3);
    host.shutdown().unwrap();
}

#[test]
fn reusable_runner_requires_the_applications_approval_callback() {
    struct Deny;
    impl arcs_host::pipeline::PipelineApproval for Deny {
        fn approve(
            &mut self,
            _: &Host,
            _: &arcs_host::pipeline::PipelineOutput<'_>,
        ) -> Result<Artifact, arcs_host::pipeline::PipelineError> {
            Err(arcs_host::pipeline::PipelineError::Policy(
                "application denied export".into(),
            ))
        }
    }
    let fixture = Fixture::new("arcs.measurement.temperature.v1", "Cel");
    let mut host = fixture.open();
    assert!(arcs_host::pipeline::run_pipeline(&mut host, &fixture.config(), &mut Deny).is_err());
    let failed = checkpoint(&host);
    assert_eq!(failed["cursor"], 0);
    assert!(!failed["active"]["request_version_id"].is_null());
    assert!(failed["active"]["approval_version_id"].is_null());
    assert!(fixture.files().is_empty());
    let resumed = pipeline::run(&mut host, &fixture.config()).unwrap();
    assert_eq!(resumed.deliveries.len(), 2);
    assert_eq!(
        json!(resumed.deliveries[0].steps[0].version_id),
        failed["active"]["step_versions"][0]
    );
    host.shutdown().unwrap();
}

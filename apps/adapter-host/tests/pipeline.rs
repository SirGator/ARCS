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

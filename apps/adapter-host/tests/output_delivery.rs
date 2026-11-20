#[path = "support/monitor.rs"]
mod monitoring;
#[path = "support/parallel.rs"]
mod parallel;
mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SourceKind, SystemClock, TrustLevel};
use arcs::input::{InputMessage, InputService};
use arcs::output::OutputError;
use arcs::runtime::{InvocationService, InvocationStatus};
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime, output::OutputCommand};
use arcs_host::{Host, HostError};
use monitoring::Monitor;
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    grant: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-host-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let config = json!({"config_version":1,"database":"db/core.sqlite","schemas":[root.join("adapters/file-output/schemas/report.schema.json"),root.join("adapters/file-output/schemas/receipt.schema.json")],
            "adapters":[{"program":support::adapter_binary("arcs-file-output"),"profile":"profile.json","args":[{"path":"db/output.sqlite"}],"grant_file":"grant.json"}]});
        std::fs::write(
            directory.join("host.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("profile.json"),
            serde_json::to_vec(&json!({"adapter_id":"example.file-output","directory":"reports"}))
                .unwrap(),
        )
        .unwrap();
        let fixture = Self {
            directory,
            grant: serde_json::from_str(include_str!("../config/grants/output.json")).unwrap(),
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        std::fs::write(
            self.directory.join("grant.json"),
            serde_json::to_vec(&self.grant).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.directory.join("reports"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
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
fn capability() -> CapabilityRef {
    CapabilityRef::new("example.file-output", "report.write")
}
fn input(host: &Host, id: &str) -> Artifact {
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            AdapterManifest {
                protocol_version: 1,
                adapter_id: AdapterId("test.operator".into()),
                adapter_version: "1".into(),
                capabilities: vec![CapabilityDescriptor {
                    description: None,
                    id: CapabilityId("report.input".into()),
                    contract: CapabilityContract::Input {
                        emits: vec![SchemaId("arcs.report.v1".into())],
                    },
                    required_permissions: vec![],
                }],
            },
            AdapterGrant {
                adapter_id: AdapterId("test.operator".into()),
                producer_class: ProducerClass::System,
                enabled_capabilities: vec![CapabilityId("report.input".into())],
                granted_permissions: vec![],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: Some(SourceKind::Internal),
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                input_data_limits: None,
                reasoning_limits_v2: None,
                reasoning_limits: None,
                data_limits: None,
            },
            host.schemas(),
            host.store(),
        )
        .unwrap();
    InputService::new(
        &registry,
        host.schemas(),
        host.store(),
        &mut host.store().durable_id_generator("test-input").unwrap(),
        &SystemClock,
    )
    .ingest(
        &AdapterId("test.operator".into()),
        InputMessage {
            capability_id: CapabilityId("report.input".into()),
            external_subject: Some(id.into()),
            external_reference: id.into(),
            payload: json!({"title":"Report","body":"Approved example"}),
        },
    )
    .unwrap()
}
fn approve(host: &Host, request: &Artifact, decision: ApprovalDecision) -> Artifact {
    let mut ids = host.store().durable_id_generator("test-policy").unwrap();
    let checked = VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .verify(&request.version_id, &Check)
        .unwrap();
    ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .decide(
            &request.version_id,
            &checked.version_id,
            decision,
            Actor {
                actor_type: ActorType::System,
                id: "test.local-policy".into(),
            },
            "explicit test decision".into(),
        )
        .unwrap()
}

#[test]
fn a_queued_output_keeps_the_monitor_live_and_replays_an_interrupted_receipt() {
    let f = parallel::Fixture::new();
    f.peer("independent", "normal", false);
    f.route("independent", "independent");
    let mut runtime = f.read("runtime.json");
    runtime["interval_ms"] = json!(15);
    f.write("runtime.json", runtime);
    let mut input_profile = f.read("input-profile.json");
    let event = input_profile["events"][0].clone();
    input_profile["events"] = json!([]);
    f.write("input-profile.json", input_profile);
    f.write("review-input.json", event);
    f.write(
        "output-profile.json",
        json!({"adapter_id":"example.file-output","directory":"reports"}),
    );
    f.write(
        "output-grant.json",
        serde_json::from_str(include_str!("../config/grants/output.json")).unwrap(),
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut config = f.read("host.json");
    for schema in ["report", "receipt"] {
        config["schemas"].as_array_mut().unwrap().push(json!(
            root.join(format!("adapters/file-output/schemas/{schema}.schema.json"))
        ));
    }
    config["adapters"].as_array_mut().unwrap().push(json!({
        "program":"/usr/bin/python3","timeout_ms":30000,"grant_file":"output-grant.json",
        "args":[{"path":"hold-receipt.py"},{"path":support::adapter_binary("arcs-file-output")},
                {"path":"output-profile.json"},{"path":"output.sqlite"}]
    }));
    f.write("host.json", config);
    std::fs::write(
        f.directory.join("hold-receipt.py"),
        r#"
import json, pathlib, subprocess, sys, time
root = pathlib.Path(__file__).parent
peer = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
try:
    for line in sys.stdin.buffer:
        peer.stdin.write(line)
        peer.stdin.flush()
        reply = peer.stdout.readline()
        if not reply:
            break
        message = json.loads(line)["message"]
        if message["type"] == "output":
            with (root / "attempts").open("a") as attempts:
                attempts.write(message["invocation"]["invocation_id"] + "\n")
            while not (root / "release-receipt").exists():
                time.sleep(.005)
        sys.stdout.buffer.write(reply)
        sys.stdout.buffer.flush()
finally:
    peer.stdin.close()
    peer.wait()
"#,
    )
    .unwrap();

    let (mut host, runtime) = f.open();
    let report = input(&host, "queued-report");
    let request = host
        .prepare_output(
            &capability(),
            &report.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    let approval = approve(&host, &request, ApprovalDecision::Approved);
    host.shutdown().unwrap();
    drop((runtime, host));

    let enqueue = json!({
        "command":"enqueue","adapter_id":capability().adapter_id,
        "request_version_id":request.version_id,"approval_version_id":approval.version_id
    });
    f.write("enqueue.json", enqueue.clone());
    let cli = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .current_dir(&f.directory)
        .args([
            "--config",
            "host.json",
            "--runtime",
            "runtime.json",
            "--output-command",
            "enqueue.json",
        ])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let receipt: Value = serde_json::from_slice(&cli.stdout).unwrap();
    let delivery = &receipt["output_delivery"];
    let id = delivery["invocation_id"].as_str().unwrap();
    let sequence = delivery["sequence"].as_u64().unwrap();
    let inspect = json!({"command":"inspect","sequence":sequence});
    let attempts = || std::fs::read_to_string(f.directory.join("attempts")).unwrap_or_default();
    assert!(attempts().is_empty());
    assert_eq!(delivery["state"], "queued");

    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| attempts().lines().count() == 1 && event["output_workers"][0] == id);
    let mut invalid = enqueue.clone();
    invalid["approval_version_id"] = json!(report.version_id);
    let rejected = monitor.command("bad-approval", "output", invalid);
    assert_eq!(rejected["ok"], false);
    assert_eq!(
        monitor.command("duplicate", "output", enqueue.clone())["receipt"],
        *delivery
    );
    assert_eq!(
        monitor.command("inspect", "output", inspect.clone())["receipt"],
        *delivery
    );
    let mut other = f.read("runtime.json");
    other["runtime_id"] = json!("second-output-runtime");
    other["journal"] = json!("second.sqlite");
    f.write("second.json", other);
    let open_second = || {
        let host = Host::open(f.directory.join("host.json")).unwrap();
        let runtime = TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("second.json")).unwrap(),
        )
        .unwrap();
        (host, runtime)
    };
    let (mut other_host, mut other_runtime) = open_second();
    let other_delivery = other_runtime
        .control_output(&mut other_host, serde_json::from_value(enqueue).unwrap())
        .unwrap();
    assert_eq!(other_delivery.invocation_id, id);
    assert!(matches!(
        other_host.deliver_output(
            &capability().adapter_id,
            &request.version_id,
            &approval.version_id
        ),
        Err(HostError::AdapterBusy(_))
    ));
    for _ in 0..3 {
        other_runtime.tick(&mut other_host).unwrap();
    }
    assert_eq!(attempts().lines().collect::<Vec<_>>(), [id]);
    other_host.shutdown().unwrap();
    drop((other_runtime, other_host));
    let submitted = Command::new(support::adapter_binary("arcs-json-input"))
        .current_dir(&f.directory)
        .args([
            "--enqueue",
            "input-profile.json",
            "input.sqlite",
            "review-input.json",
        ])
        .output()
        .unwrap();
    assert!(
        submitted.status.success(),
        "{}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let snapshot = monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    assert_eq!(snapshot["output_workers"][0], id);
    assert_eq!(attempts().lines().collect::<Vec<_>>(), [id]);
    monitor.stop();
    std::fs::remove_file(f.directory.join("monitor.stop")).unwrap();
    std::fs::write(f.directory.join("release-receipt"), "continue").unwrap();

    for (database, trigger, expected) in [
        (
            "core.sqlite",
            "CREATE TRIGGER lose_receipt BEFORE INSERT ON artifact_versions
             WHEN NEW.schema_id='arcs.delivery_receipt.file.v1' BEGIN SELECT RAISE(ABORT,'test receipt rollback'); END;",
            InvocationStatus::Dispatched,
        ),
        (
            "runtime.sqlite",
            "CREATE TRIGGER lose_receipt BEFORE UPDATE ON runtime_output_deliveries
             WHEN NEW.state='delivered' BEGIN SELECT RAISE(ABORT,'test queue rollback'); END;",
            InvocationStatus::Succeeded,
        ),
    ] {
        let (mut host, mut runtime) = f.open();
        let db = rusqlite::Connection::open(f.directory.join(database)).unwrap();
        db.execute_batch(trigger).unwrap();
        support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host).map_err(|error| match error {
            HostError::Output { source: OutputError::Invocation(arcs::runtime::InvocationError::Store(source)), .. } => HostError::Store(source),
            other => other,
        }));
        assert_eq!(InvocationService::new(host.store(), host.schemas(), &SystemClock)
            .lookup(id).unwrap().unwrap().status, expected);
        assert_eq!(json!(runtime.control_output(&mut host, OutputCommand::Inspect{sequence}).unwrap()), *delivery);
        db.execute_batch("DROP TRIGGER lose_receipt").unwrap();
        host.shutdown().unwrap();
    }

    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| event["runtime"]["output_deliveries"][0]["state"] == "delivered");
    let delivery = monitor.command("inspect-final", "output", inspect)["receipt"].clone();
    assert_eq!(delivery["invocation_id"], id);
    assert!(delivery["receipt_version_id"].is_string());
    assert_eq!(attempts().lines().collect::<Vec<_>>(), [id, id, id]);
    assert_eq!(f.count("output", "deliveries"), 1);
    assert_eq!(
        std::fs::read_dir(f.directory.join("reports"))
            .unwrap()
            .count(),
        1
    );
    monitor.stop();

    let (mut other_host, mut other_runtime) = open_second();
    other_runtime.tick(&mut other_host).unwrap();
    let other_final = other_runtime
        .control_output(
            &mut other_host,
            OutputCommand::Inspect {
                sequence: other_delivery.sequence,
            },
        )
        .unwrap();
    assert_eq!(
        json!(other_final.receipt_version_id),
        delivery["receipt_version_id"]
    );
    assert_eq!(attempts().lines().collect::<Vec<_>>(), [id, id, id]);
    other_host.shutdown().unwrap();
}

#[test]
fn real_output_process_requires_approval_and_replays_without_duplicate_files() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.open().run().unwrap().stored_artifacts, 0);
    assert!(fixture.files().is_empty());
    let mut host = fixture.open();
    let input = input(&host, "report-1");
    let request = host
        .prepare_output(
            &capability(),
            &input.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    assert!(fixture.files().is_empty());
    let approval = approve(&host, &request, ApprovalDecision::Approved);
    let receipt = host
        .deliver_output(
            &capability().adapter_id,
            &request.version_id,
            &approval.version_id,
        )
        .unwrap();
    let count = host.report().unwrap().stored_artifacts;
    assert_eq!(fixture.files().len(), 1);
    let file: Value = serde_json::from_slice(&std::fs::read(&fixture.files()[0]).unwrap()).unwrap();
    assert_eq!(file["payload"], input.payload);
    assert_eq!(file["approval_version_id"], json!(approval.version_id));
    assert_eq!(
        host.deliver_output(
            &capability().adapter_id,
            &request.version_id,
            &approval.version_id
        )
        .unwrap(),
        receipt
    );
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    assert_eq!(
        host.deliver_output(
            &capability().adapter_id,
            &request.version_id,
            &approval.version_id
        )
        .unwrap(),
        receipt
    );
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    assert_eq!(host.report().unwrap().world_revision, 0);
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
    drop(host);
    fixture.grant["producer_class"] = json!("adapter");
    fixture.save();
    let mut host = fixture.open();
    assert!(matches!(
        host.deliver_output(
            &capability().adapter_id,
            &request.version_id,
            &approval.version_id
        ),
        Err(HostError::Output {
            source: OutputError::ExecutorRequired,
            ..
        })
    ));
    assert_eq!(fixture.files().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn rejected_and_different_report_approvals_do_not_publish() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let first = input(&host, "report-1");
    let first = host
        .prepare_output(
            &capability(),
            &first.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    let rejected = approve(&host, &first, ApprovalDecision::Rejected);
    assert!(matches!(
        host.deliver_output(
            &capability().adapter_id,
            &first.version_id,
            &rejected.version_id
        ),
        Err(HostError::Output {
            source: OutputError::Gate(_),
            ..
        })
    ));
    let approved = approve(&host, &first, ApprovalDecision::Approved);
    let second = input(&host, "report-2");
    let second = host
        .prepare_output(
            &capability(),
            &second.version_id,
            &SchemaId("arcs.delivery_receipt.file.v1".into()),
        )
        .unwrap();
    assert!(matches!(
        host.deliver_output(
            &capability().adapter_id,
            &second.version_id,
            &approved.version_id
        ),
        Err(HostError::Output {
            source: OutputError::Gate(_),
            ..
        })
    ));
    assert!(fixture.files().is_empty());
    host.shutdown().unwrap();
}

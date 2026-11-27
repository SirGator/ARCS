#[path = "support/monitor.rs"]
mod monitoring;
#[path = "support/parallel.rs"]
mod parallel;
#[path = "support/plans.rs"]
mod plans;
mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
use parallel as parallel_support;
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
                data_classification: None,
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

fn file_output_host(f: &parallel::Fixture) -> Value {
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
    config
}

#[test]
fn a_task_result_requires_exact_review_and_recovers_its_approval_after_queue_rollback() {
    let f = parallel::Fixture::new();
    f.peer("numeric", "normal", false);
    f.route("analysis", "numeric");
    f.route("follow-up", "numeric");
    let mut config = file_output_host(&f);
    config["adapters"].as_array_mut().unwrap().push(json!({
        "program":support::adapter_binary("arcs-file-output"),"profile":"output-profile.json",
        "args":[{"path":"output.sqlite"}],"grant_file":"output-grant.json"
    }));
    f.write("host.json", config);
    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| event["runtime"]["ready_for_review"] == 2);
    let mut targets = Vec::new();
    for sequence in [1, 2] {
        let options = monitor.command(
            &format!("options-{sequence}"),
            "task_output",
            json!({"command":"options","task_sequence":sequence}),
        );
        assert_eq!(options["ok"], true, "{options}");
        let options = &options["receipt"];
        let mut target = options["destinations"][0].clone();
        target["task_sequence"] = json!(sequence);
        target["result_version_id"] = options["result_versions"][0].clone();
        targets.push(target);
    }
    assert_ne!(
        targets[0]["result_version_id"],
        targets[1]["result_version_id"]
    );
    let preview = |monitor: &mut Monitor, id, target: &Value| {
        let response = monitor.command(
            id,
            "task_output",
            json!({"command":"preview","target":target}),
        );
        assert_eq!(response["ok"], true, "{response}");
        response["receipt"].clone()
    };
    let first = preview(&mut monitor, "preview-1", &targets[0]);
    assert_eq!(
        preview(&mut monitor, "preview-1-replay", &targets[0]),
        first
    );
    let second = preview(&mut monitor, "preview-2", &targets[1]);
    let contents: Value = serde_json::from_str(first["payload"]["body"].as_str().unwrap()).unwrap();
    assert!(contents["value"].is_number());
    assert!(contents.get("private_note").is_none());
    assert_eq!(f.count("output", "deliveries"), 0);
    let review = |preview: &Value, decision| {
        json!({"command":"review","review":{
            "target":preview["target"],"request_version_id":preview["request_version_id"],
            "verification_version_id":preview["verification_version_id"],"operator_id":"local-operator",
            "decision":decision,"reason":"Reviewed this exact report and destination"
        }})
    };
    let approve = review(&first, "approved");
    let mut wrong = approve.clone();
    wrong["review"]["verification_version_id"] = second["verification_version_id"].clone();
    assert_eq!(
        monitor.command("wrong-verification", "task_output", wrong)["ok"],
        false
    );
    let mut wrong = approve.clone();
    wrong["review"]["target"]["result_version_id"] = targets[1]["result_version_id"].clone();
    assert_eq!(
        monitor.command("wrong-result", "task_output", wrong)["ok"],
        false
    );
    let rejected = monitor.command("reject-2", "task_output", review(&second, "rejected"));
    assert_eq!(rejected["receipt"]["state"], "pending", "{rejected}");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let rejected = monitor.command(
            "inspect-2",
            "task_output",
            json!({"command":"inspect","request_version_id":second["request_version_id"]}),
        );
        if rejected["receipt"]["state"] == "rejected" {
            assert!(rejected["receipt"]["approval_version_id"].is_string());
            assert!(rejected["receipt"]["delivery_sequence"].is_null());
            break;
        }
        assert!(Instant::now() < deadline, "{rejected}");
    }
    assert_eq!(f.count("output", "deliveries"), 0);
    monitor.stop();
    std::fs::remove_file(f.directory.join("monitor.stop")).unwrap();

    f.write("review.json", approve.clone());
    let cli = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .current_dir(&f.directory)
        .args([
            "--config",
            "host.json",
            "--runtime",
            "runtime.json",
            "--task-output-command",
            "review.json",
        ])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let accepted: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(accepted["task_output"]["state"], "pending");

    let (mut host, mut runtime) = f.open();
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    journal
        .execute_batch(
            "CREATE TRIGGER lose_admission BEFORE INSERT ON runtime_output_deliveries
         BEGIN SELECT RAISE(ABORT, 'simulated output admission rollback'); END;",
        )
        .unwrap();
    assert!(matches!(runtime.tick(&mut host), Err(HostError::Store(_))));
    let approval = host
        .store()
        .find_by_stream_key(&format!(
            "approval:{}",
            first["request_version_id"].as_str().unwrap()
        ))
        .unwrap()
        .unwrap();
    assert_eq!(approval.payload["decision"], "approved");
    assert_eq!(f.count("output", "deliveries"), 0);
    journal
        .execute_batch("DROP TRIGGER lose_admission")
        .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let mut monitor = Monitor::start(&f.directory);
    let completed =
        monitor.until(|event| event["runtime"]["output_deliveries"][0]["state"] == "delivered");
    let delivery = &completed["runtime"]["output_deliveries"][0];
    let replay = monitor.command("approve-replay", "task_output", approve);
    assert_eq!(replay["receipt"]["state"], "queued");
    assert_eq!(
        replay["receipt"]["approval_version_id"],
        json!(approval.version_id)
    );
    assert_eq!(replay["receipt"]["delivery_sequence"], delivery["sequence"]);
    assert_eq!(
        monitor.command(
            "conflicting-decision",
            "task_output",
            review(&first, "rejected")
        )["ok"],
        false
    );
    let files = std::fs::read_dir(f.directory.join("reports"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let file: Value = serde_json::from_slice(&std::fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(file["payload"], first["payload"]);
    assert_eq!(f.count("output", "deliveries"), 1);
    monitor.stop();
}

#[test]
fn a_successful_plan_revision_prepares_one_exact_report_after_checkpoint_rollback() {
    let f = plans::fixture("fail", "normal");
    let mut host_config = file_output_host(&f);
    host_config["adapters"].as_array_mut().unwrap().push(json!({
        "program":support::adapter_binary("arcs-file-output"),"profile":"output-profile.json",
        "args":[{"path":"output.sqlite"}],"grant_file":"output-grant.json"
    }));
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["routes"][0]["budget"]["max_context_items"] = json!(4);
    config["routes"][0]["plan"]["replanning"] = json!({"max_revisions":1});
    config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(100000);
    config["routes"][0]["plan"]["templates"][0]["route"]["budget"]["max_context_items"] = json!(2);
    config["routes"][0]["plan"]["templates"][1]["output"] = json!({
        "destination":capability(),"receipt_schema_id":"arcs.delivery_receipt.file.v1"
    });
    f.write("runtime.json", config);
    f.write(
        "planner.json",
        json!({
            "proposal":{"steps":[{"id":"done","template":"b"},{"id":"fails","template":"a",
                "dependencies":[{"step_id":"done","payload_fields":["value"]}]}]},
            "revisions":{"1":{"steps":[{"id":"replacement","template":"b"}]}}
        }),
    );

    let (mut host, mut runtime) = f.open();
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    journal
        .execute_batch(
            "CREATE TRIGGER lose_draft BEFORE INSERT ON runtime_plan_outputs
         BEGIN SELECT RAISE(ABORT, 'simulated draft insertion rollback'); END;",
        )
        .unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host));
    assert_eq!(f.count("runtime", "runtime_plan_outputs"), 0);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(
        plan.state,
        arcs_host::task_runtime::planning::PlanState::Running
    );
    let superseded = plan.steps.iter().find(|step| step.revision == 0).unwrap();
    let replacement = plan.steps.iter().find(|step| step.revision == 1).unwrap();
    assert_eq!(superseded.state, "ready_for_review");
    assert_eq!(replacement.state, "ready_for_review");

    journal
        .execute_batch(
            "DROP TRIGGER lose_draft;
         CREATE TRIGGER lose_preview BEFORE UPDATE OF state ON runtime_plan_outputs
         WHEN NEW.state='ready'
         BEGIN SELECT RAISE(ABORT, 'simulated proof checkpoint rollback'); END;",
        )
        .unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host));
    assert_eq!(f.count("runtime", "runtime_plan_outputs"), 1);
    let requests = plans::artifacts(&host, "arcs.output_request.v1");
    assert_eq!(requests.len(), 1);
    let request = &requests[0].version_id;
    let verification = host
        .store()
        .find_by_stream_key(&format!("verification:{}", request.0))
        .unwrap()
        .unwrap()
        .version_id;
    assert_eq!(f.count("output", "deliveries"), 0);
    journal.execute_batch("DROP TRIGGER lose_preview").unwrap();
    host.shutdown().unwrap();
    drop((runtime, host, journal));

    let mut monitor = Monitor::start(&f.directory);
    let deadline = Instant::now() + Duration::from_secs(25);
    let draft = loop {
        let response = monitor.command(
            "plan-output-options",
            "task_output",
            json!({"command":"options","task_sequence":1}),
        );
        assert_eq!(response["ok"], true, "{response}");
        let drafts = response["receipt"]["plan_outputs"].as_array().unwrap();
        assert_eq!(drafts.len(), 1);
        if drafts[0]["state"] == "ready" {
            break drafts[0].clone();
        }
        assert_eq!(drafts[0]["state"], "preparing", "{response}");
        assert!(Instant::now() < deadline, "{response}");
    };
    assert_eq!(draft["spec"]["task_sequence"], replacement.sequence);
    assert!(draft["spec"].get("export_fields").is_none());
    assert_eq!(draft["request_version_id"], json!(request));
    assert_eq!(draft["verification_version_id"], json!(verification));
    let response = monitor.command(
        "plan-output-preview",
        "task_output",
        json!({"command":"preview_plan_output","sequence":draft["sequence"]}),
    );
    assert_eq!(response["ok"], true, "{response}");
    let preview = &response["receipt"];
    assert_eq!(
        preview["verification_version_id"],
        draft["verification_version_id"]
    );
    let contents: Value =
        serde_json::from_str(preview["payload"]["body"].as_str().unwrap()).unwrap();
    assert_eq!(contents.as_object().unwrap().len(), 1, "{contents}");
    assert!(contents["value"].is_number());
    assert_eq!(f.count("output", "deliveries"), 0);

    let review = json!({"command":"review","review":{
        "target":preview["target"],"request_version_id":request,
        "verification_version_id":preview["verification_version_id"],"operator_id":"local-operator",
        "decision":"approved","reason":"Reviewed the configured plan report"
    }});
    let mut changed = review.clone();
    changed["review"]["target"]["destination"]["capability_id"] = json!("other.write");
    assert_eq!(
        monitor.command("changed-plan-target", "task_output", changed)["ok"],
        false
    );
    assert_eq!(f.count("output", "deliveries"), 0);
    assert_eq!(
        monitor.command("approved-plan-output", "task_output", review)["ok"],
        true
    );
    monitor.until(|event| event["runtime"]["output_deliveries"][0]["state"] == "delivered");
    let files = std::fs::read_dir(f.directory.join("reports"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let file: Value = serde_json::from_slice(&std::fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(file["payload"], preview["payload"]);
    assert_eq!(f.count("output", "deliveries"), 1);
    monitor.stop();
    let (mut host, _) = f.open();
    assert_eq!(plans::artifacts(&host, "arcs.output_request.v1").len(), 1);
    host.shutdown().unwrap();
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
    let mut config = file_output_host(&f);
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

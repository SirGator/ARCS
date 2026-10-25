mod support;
use arcs::action::ActionService;
use arcs::adapters::{AdapterId, AdapterRole, CapabilityRef};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SystemClock, TrustLevel};
use arcs::decision::proposal::{CandidateProposal, ProposalService};
use arcs::reasoning::{ContextSelection, ReasoningMethod, ReasoningRequestV2, ReasoningResources};
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs_host::Host;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-multiple-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let source = root.join("apps/adapter-host/config/multiple-adapters");
        for file in std::fs::read_dir(&source).unwrap() {
            let file = file.unwrap();
            if file.path().extension().is_some_and(|ext| ext == "json") {
                std::fs::copy(file.path(), directory.join(file.file_name())).unwrap();
            }
        }
        let fixture = Self { directory };
        let mut config = fixture.read("host.json");
        config["database"] = json!("db/core.sqlite");
        for schema in config["schemas"].as_array_mut().unwrap() {
            *schema = json!(root.join(schema.as_str().unwrap().trim_start_matches("../../../../")));
        }
        for adapter in config["adapters"].as_array_mut().unwrap() {
            let name = PathBuf::from(adapter["program"].as_str().unwrap());
            adapter["program"] = json!(support::adapter_binary(
                name.file_name().unwrap().to_str().unwrap()
            ));
            adapter["timeout_ms"] = json!(15000);
            for arg in adapter
                .get_mut("args")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if let Some(path) = arg.get_mut("path") {
                    let filename = PathBuf::from(path.as_str().unwrap())
                        .file_name()
                        .unwrap()
                        .to_os_string();
                    *path = json!(PathBuf::from("db").join(filename));
                }
            }
        }
        for side in ["left", "right"] {
            let mut profile = fixture.read(&format!("output-{side}.json"));
            profile["directory"] = json!(format!("reports-{side}"));
            fixture.write(&format!("output-{side}.json"), &profile);
        }
        fixture.write("host.json", &config);
        fixture
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn write(&self, name: &str, value: &Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn catalog_lists_every_external_role_without_polling_and_cli_preserves_filtering() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .arg("--config")
        .arg(fixture.directory.join("host.json"))
        .arg("--list-adapters")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(catalog.as_array().unwrap().len(), 11);
    let mut host = fixture.open();
    assert_eq!(
        serde_json::to_value(host.adapter_catalog()).unwrap(),
        catalog
    );
    assert_eq!(host.report().unwrap().stored_artifacts, 0);
    assert_eq!(host.report().unwrap().world_revision, 0);
    for role in [
        AdapterRole::Input,
        AdapterRole::Observe,
        AdapterRole::Request,
        AdapterRole::Transform,
        AdapterRole::Reason,
        AdapterRole::Act,
        AdapterRole::Output,
    ] {
        let capabilities = host.capabilities(role);
        assert!(capabilities.len() >= 2, "{role:?}");
        assert!(
            capabilities
                .iter()
                .all(|cap| cap.adapter_id.0.starts_with("multi."))
        );
    }
    assert_eq!(
        host.capabilities(AdapterRole::Reason),
        vec![
            CapabilityRef::new("multi.reasoner.model", "solution.propose"),
            CapabilityRef::new("multi.reasoner.solver", "solution.propose")
        ]
    );
    let methods = host
        .adapter_catalog()
        .into_iter()
        .find(|a| a.adapter_id.0 == "multi.reasoner.solver")
        .unwrap()
        .capabilities[0]
        .reasoning_methods
        .clone();
    assert_eq!(methods, vec![ReasoningMethod::Solver]);
    host.shutdown().unwrap();
}

fn reason(input: &Artifact, model: bool) -> ReasoningRequestV2 {
    ReasoningRequestV2 {
        request_id: format!("same-task:{}", input.version_id.0),
        capability: CapabilityRef::new(
            if model {
                "multi.reasoner.model"
            } else {
                "multi.reasoner.solver"
            },
            "solution.propose",
        ),
        method: if model {
            ReasoningMethod::Learned
        } else {
            ReasoningMethod::Solver
        },
        objective: "Find nearest value".into(),
        context: vec![ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["values".into(), "target".into()],
        }],
        target_schema_id: SchemaId("arcs.solution_candidate.nearest.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: ReasoningResources {
            max_context_items: 1,
            max_context_bytes: 8192,
            max_output_bytes: 4096,
            max_candidates: 1,
            max_runtime_ms: 1000,
            max_iterations: if model { None } else { Some(4096) },
            max_output_tokens: None,
        },
    }
}
#[test]
fn two_input_sources_and_two_reasoners_keep_identical_local_names_independent() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let mut inputs = host.poll_inputs().unwrap();
    inputs.sort_by(|a, b| a.created_by.id.cmp(&b.created_by.id));
    assert_eq!(inputs.len(), 2);
    assert_ne!(inputs[0].version_id, inputs[1].version_id);
    assert_ne!(inputs[0].subject, inputs[1].subject);
    let mut results = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        for model in [false, true] {
            let result = host.reason(reason(input, model)).unwrap();
            let expected = if model {
                if index == 0 { 1.0 } else { 10.0 }
            } else if index == 0 {
                2.0
            } else {
                20.0
            };
            assert_eq!(result.candidates[0].payload["value"], json!(expected));
            assert_eq!(
                result.candidates[0].trust.level,
                if model {
                    TrustLevel::Low
                } else {
                    TrustLevel::Medium
                }
            );
            results.push((input.clone(), model, result));
        }
    }
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    assert!(host.poll_inputs().unwrap().is_empty());
    for (input, model, first) in results {
        let replay = host.reason(reason(&input, model)).unwrap();
        assert_eq!(first.candidates, replay.candidates);
        assert_eq!(first.result_version_id, replay.result_version_id);
    }
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    host.shutdown().unwrap();
}

struct TestPolicy;
impl Verifier for TestPolicy {
    fn verify(&self, target: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: if target.schema_id.0 == "arcs.fan_candidate.v1"
                || target.schema_id.0 == "arcs.output_request.v1"
            {
                VerificationVerdict::Pass
            } else {
                VerificationVerdict::Fail
            },
            findings: vec![],
        })
    }
}
fn approve(host: &Host, target: &Artifact) -> Artifact {
    let mut ids = host
        .store()
        .durable_id_generator("multi-test-policy")
        .unwrap();
    let report = VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .verify(&target.version_id, &TestPolicy)
        .unwrap();
    ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .decide(
            &target.version_id,
            &report.version_id,
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::System,
                id: "test.multiple-adapters.policy".into(),
            },
            "explicit simulation test decision".into(),
        )
        .unwrap()
}
#[test]
fn requests_transforms_actions_and_outputs_target_the_selected_instance_and_replay() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let mut replays = Vec::new();
    for (side, value) in [("left", 31.0), ("right", 41.0)] {
        let fan = AdapterId(format!("multi.fan.{side}"));
        let other = AdapterId(format!(
            "multi.fan.{}",
            if side == "left" { "right" } else { "left" }
        ));
        let temperature = host.request(&format!("temperature-{side}")).unwrap();
        assert_eq!(temperature.payload["value"], json!(value));
        assert_eq!(temperature.created_by.id, fan.0);
        let normalized = host
            .transform_configured(&format!("normalize-{side}"))
            .unwrap();
        assert_eq!(normalized.payload["value"], json!(value));
        assert_eq!(normalized.created_by.id, format!("multi.normalize.{side}"));
        let report = host
            .transform(
                &CapabilityRef::new("multi.report", "json.report"),
                &normalized.version_id,
                &SchemaId("arcs.report.v1".into()),
            )
            .unwrap();
        let output = CapabilityRef::new(format!("multi.output.{side}"), "report.write");
        let request = host
            .prepare_output(
                &output,
                &report.version_id,
                &SchemaId("arcs.delivery_receipt.file.v1".into()),
            )
            .unwrap();
        let approval = approve(&host, &request);
        let wrong_output = AdapterId(format!(
            "multi.output.{}",
            if side == "left" { "right" } else { "left" }
        ));
        assert!(
            host.deliver_output(&wrong_output, &request.version_id, &approval.version_id)
                .is_err()
        );
        let receipt = host
            .deliver_output(
                &output.adapter_id,
                &request.version_id,
                &approval.version_id,
            )
            .unwrap();
        assert_eq!(receipt.created_by.id, output.adapter_id.0);
        let cap = CapabilityRef::new(&fan.0, "fan.set");
        let mut ids = host
            .store()
            .durable_id_generator("multi-candidate")
            .unwrap();
        let candidate = ProposalService::new(
            host.registry(),
            host.schemas(),
            host.store(),
            &mut ids,
            &SystemClock,
        )
        .propose(CandidateProposal {
            proposal_id: format!("fan-{side}"),
            rule_id: "test.enable-one-simulated-fan".into(),
            schema_id: SchemaId("arcs.fan_candidate.v1".into()),
            payload: json!({"device_id":"fan-01","enabled":true}),
            evidence: vec![temperature.version_id],
            required_capabilities: vec![cap.clone()],
        })
        .unwrap();
        let action_approval = approve(&host, &candidate);
        let action = ActionService::new(
            host.registry(),
            host.schemas(),
            host.store(),
            &mut ids,
            &SystemClock,
        )
        .materialize(&action_approval.version_id, &cap)
        .unwrap();
        assert!(host.execute_approved(&other, &action.version_id).is_err());
        let other_side = if side == "left" { "right" } else { "left" };
        let other_count: i64 = rusqlite::Connection::open(
            fixture
                .directory
                .join(format!("db/fan-{other_side}.sqlite")),
        )
        .unwrap()
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap();
        assert_eq!(other_count, if side == "left" { 0 } else { 1 });
        let effect = host.execute_approved(&fan, &action.version_id).unwrap();
        assert_eq!(effect.payload["result"]["actuation_count"], 1);
        assert_eq!(effect.created_by.id, fan.0);
        replays.push((fan, action, effect, output, request, approval, receipt));
    }
    // Both device profiles deliberately use fan-01. Observations still have
    // distinct Core subjects because the adapter installation is part of them.
    host.poll_round().unwrap();
    let states = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.artifact.schema_id.0 == "arcs.state.fan.v1")
        .map(|e| e.artifact)
        .collect::<Vec<_>>();
    let left = states
        .iter()
        .find(|a| a.created_by.id == "multi.fan.left")
        .unwrap();
    let right = states
        .iter()
        .find(|a| a.created_by.id == "multi.fan.right")
        .unwrap();
    assert_ne!(left.subject, right.subject);
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    for (fan, action, effect, output, request, approval, receipt) in replays {
        assert_eq!(
            host.execute_approved(&fan, &action.version_id).unwrap(),
            effect
        );
        assert_eq!(
            host.deliver_output(
                &output.adapter_id,
                &request.version_id,
                &approval.version_id
            )
            .unwrap(),
            receipt
        );
    }
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    for side in ["left", "right"] {
        assert_eq!(
            std::fs::read_dir(fixture.directory.join(format!("reports-{side}")))
                .unwrap()
                .count(),
            1
        );
    }
    host.shutdown().unwrap();
}

#[test]
fn duplicate_installation_ids_and_incompatible_cli_modes_fail_before_startup() {
    let fixture = Fixture::new();
    let mut grant = fixture.read("input-right-grant.json");
    grant["adapter_id"] = json!("multi.input.left");
    fixture.write("input-right-grant.json", &grant);
    assert!(Host::open(fixture.directory.join("host.json")).is_err());
    assert!(!fixture.directory.join("db").exists());
    for flags in [
        vec!["--list-adapters", "--list-adapters"],
        vec!["--list-adapters", "--runtime", "missing.json"],
        vec!["--list-adapters", "--ticks", "1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(fixture.directory.join("host.json"))
            .args(flags)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
        assert!(!fixture.directory.join("db").exists());
    }
}

#[test]
fn monitor_streams_real_catalog_and_snapshots_without_creating_effects() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .arg("--config")
        .arg(fixture.directory.join("host.json"))
        .args(["--monitor", "--ticks", "2", "--stop-file"])
        .arg(fixture.directory.join("stop"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events[0]["event"], "ready");
    assert_eq!(events[0]["catalog"].as_array().unwrap().len(), 11);
    let contracts = events[0]["input_contracts"].as_array().unwrap();
    assert_eq!(contracts.len(), 2);
    assert!(contracts.iter().all(
        |contract| contract["schema"]["$id"] == "arcs.task.nearest.v1"
            && contract["max_payload_bytes"].as_u64().unwrap() > 0
    ));
    let snapshot = events
        .iter()
        .rev()
        .find(|event| event["event"] == "snapshot")
        .unwrap();
    assert_eq!(snapshot["ticks"], 2);
    assert!(snapshot["stored_artifacts"].as_u64().unwrap() > 0);
    assert!(snapshot["runtime"].is_null());
    assert_eq!(events.last().unwrap()["event"], "stopped");
    assert!(
        snapshot["recent_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("payload").is_none())
    );
    for side in ["left", "right"] {
        let count: i64 =
            rusqlite::Connection::open(fixture.directory.join(format!("db/fan-{side}.sqlite")))
                .unwrap()
                .query_row("SELECT actuation_count FROM fan_state", [], |row| {
                    row.get(0)
                })
                .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            std::fs::read_dir(fixture.directory.join(format!("reports-{side}")))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn runtime_monitor_reports_durable_review_counts_and_replays_after_restart() {
    let fixture = Fixture::new();
    let mut runtime: Value =
        serde_json::from_str(include_str!("../config/roles/runtime.json")).unwrap();
    runtime["runtime_id"] = json!("desktop-runtime");
    runtime["journal"] = json!("db/runtime.sqlite");
    runtime["routes"][0]["source"]["adapter_id"] = json!("multi.input.left");
    runtime["routes"][0]["reasoner"]["adapter_id"] = json!("multi.reasoner.solver");
    fixture.write("runtime.json", &runtime);
    let mut prior = None;
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(fixture.directory.join("host.json"))
            .args(["--monitor", "--ticks", "2", "--stop-file"])
            .arg(fixture.directory.join("stop"))
            .arg("--runtime")
            .arg(fixture.directory.join("runtime.json"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let snapshot = events
            .iter()
            .rev()
            .find(|event| event["event"] == "snapshot")
            .unwrap();
        assert_eq!(snapshot["runtime"]["ready_for_review"], 1);
        assert_eq!(snapshot["runtime"]["pending_jobs"], 0);
        if let Some(prior) = prior {
            assert_eq!(snapshot["stored_artifacts"], prior);
        }
        prior = Some(snapshot["stored_artifacts"].clone());
    }
}

#[test]
fn invalid_monitor_start_does_not_create_a_store_or_remove_an_existing_stop_file() {
    let fixture = Fixture::new();
    let stop = fixture.directory.join("stop");
    std::fs::write(&stop, b"keep").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .arg("--config")
        .arg(fixture.directory.join("host.json"))
        .arg("--monitor")
        .arg("--stop-file")
        .arg(&stop)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert!(!fixture.directory.join("db").exists());
    assert_eq!(std::fs::read(stop).unwrap(), b"keep");
    for args in [
        vec!["--monitor"],
        vec!["--monitor", "--monitor", "--stop-file", "missing"],
        vec!["--monitor", "--list-adapters", "--stop-file", "missing"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(fixture.directory.join("host.json"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
    assert!(!fixture.directory.join("db").exists());
}

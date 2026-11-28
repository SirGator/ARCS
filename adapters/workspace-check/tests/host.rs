#[path = "host/edit.rs"]
mod edit;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::action::ActionService;
use arcs::adapters::{AdapterId, AdapterRegistry, CapabilityId, CapabilityRef};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SystemClock, VersionId};
use arcs::decision::proposal::{CandidateProposal, ProposalService};
use arcs::input::{InputMessage, InputService};
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs_host::Host;
use rusqlite::Connection;
use serde_json::{Value, json};

const ADAPTER: &str = "test.workspace";
const TOOL: &str = "arcs.execution_result.workspace_check.v1";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let fixture = Self(std::env::temp_dir().join(format!(
            "arcs-workspace-check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir_all(fixture.0.join("project")).unwrap();
        let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
        fixture.write("host.json", json!({
            "config_version":1,"database":"core.sqlite",
            "schemas":[schemas.join("candidate.schema.json"),schemas.join("execution.schema.json")],
            "adapters":[{"program":env!("CARGO_BIN_EXE_arcs-workspace-check"),"profile":"profile.json",
                "args":[{"path":"checks.sqlite"}],"grant_file":"grant.json","timeout_ms":10000}]
        }));
        fixture.write("grant.json", json!({
            "adapter_id":ADAPTER,"producer_class":"executor","enabled_capabilities":["check.run"],
            "granted_permissions":["workspace.check"],"assigned_trust":"medium",
            "max_payload_bytes":4096,"max_external_reference_bytes":256
        }));
        fixture.write("profile.json", json!({
            "adapter_id":ADAPTER,"workspace":"project","checks":{
                "pass":{"program":"/usr/bin/python3","args":["-I","-c",
                    "with open('attempts','a') as f: f.write('pass\\n')"],"timeout_ms":2000},
                "fail":{"program":"/usr/bin/python3","args":["-I","-c","raise SystemExit(1)"],"timeout_ms":2000},
                "slow":{"program":"/usr/bin/python3","args":["-I","-c","import time; time.sleep(10)"],
                    "timeout_ms":100}
            }
        }));
        fixture
    }
    fn open(&self) -> Host {
        Host::open(self.0.join("host.json")).unwrap()
    }
    fn write(&self, filename: &str, data: Value) {
        fs::write(self.0.join(filename), serde_json::to_vec(&data).unwrap()).unwrap();
    }
    fn read(&self, filename: &str) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(filename)).unwrap()).unwrap()
    }
    fn attempts(&self) -> usize {
        fs::read_to_string(self.0.join("project/attempts"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct PassedPolicy;
impl Verifier for PassedPolicy {
    fn verify(&self, _: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: VerificationVerdict::Pass,
            findings: vec![],
        })
    }
}

fn candidate(host: &Host, proposal: &str, check: &str) -> Artifact {
    propose(
        host,
        proposal,
        "arcs.workspace_check_candidate.v1",
        "check.run",
        json!({"check":check}),
    )
}

fn propose(
    host: &Host,
    proposal: &str,
    schema: &str,
    capability: &str,
    payload: Value,
) -> Artifact {
    let mut registry = AdapterRegistry::new();
    registry.register(
        serde_json::from_value(json!({
            "protocol_version":1,"adapter_id":"test.operator","adapter_version":"1",
            "capabilities":[{"id":"task","contract":{"kind":"input","emits":["arcs.input.v1"]},"required_permissions":[]}]
        })).unwrap(),
        serde_json::from_value(json!({
            "adapter_id":"test.operator","producer_class":"system","enabled_capabilities":["task"],
            "granted_permissions":[],"assigned_trust":"high","ingress_source_kind":"internal",
            "max_payload_bytes":4096,"max_external_reference_bytes":256
        })).unwrap(),
        host.schemas(),host.store(),
    ).unwrap();
    let mut ids = host.store().durable_id_generator("test-workspace").unwrap();
    let task = InputService::new(
        &registry,
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .ingest(
        &AdapterId("test.operator".into()),
        InputMessage {
            capability_id: CapabilityId("task".into()),
            external_subject: Some(proposal.into()),
            external_reference: proposal.into(),
            payload: json!({"raw_text":"Check the local project"}),
        },
    )
    .unwrap();
    ProposalService::new(
        host.registry(),
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .propose(CandidateProposal {
        proposal_id: proposal.into(),
        rule_id: "test.workspace.v1".into(),
        schema_id: SchemaId(schema.into()),
        payload,
        evidence: vec![task.version_id],
        required_capabilities: vec![CapabilityRef::new(ADAPTER, capability)],
    })
    .unwrap()
}

fn approve(host: &Host, candidate: &Artifact) -> VersionId {
    approve_for(host, candidate, "check.run")
}

fn approve_for(host: &Host, candidate: &Artifact, capability: &str) -> VersionId {
    let mut ids = host.store().durable_id_generator("test-workspace").unwrap();
    let verification =
        VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
            .verify(&candidate.version_id, &PassedPolicy)
            .unwrap();
    let approval = ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
        .decide(
            &candidate.version_id,
            &verification.version_id,
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "test-operator".into(),
            },
            "Run this configured local check".into(),
        )
        .unwrap();
    ActionService::new(
        host.registry(),
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .materialize(
        &approval.version_id,
        &CapabilityRef::new(ADAPTER, capability),
    )
    .unwrap()
    .version_id
}

#[test]
fn a_real_check_runs_only_after_approval_and_replays_after_an_interrupted_core_commit() {
    let f = Fixture::new();
    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    let candidate = candidate(&host, "approved-pass", "pass");
    assert!(
        host.execute_approved(&adapter, &candidate.version_id, "task:test")
            .is_err()
    );
    assert_eq!(f.attempts(), 0);
    let action = approve(&host, &candidate);
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    core.execute_batch(&format!(
        "CREATE TRIGGER lose_receipt BEFORE INSERT ON artifact_versions
         WHEN NEW.schema_id='{TOOL}' BEGIN SELECT RAISE(ABORT,'simulated Core receipt rollback'); END;"
    )).unwrap();
    assert!(
        host.execute_approved(&adapter, &action, "task:test")
            .is_err()
    );
    assert_eq!(f.attempts(), 1);
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let saved: String = journal
        .query_row("SELECT result FROM checks", [], |row| row.get(0))
        .unwrap();
    host.shutdown().unwrap();
    drop(host);
    core.execute_batch("DROP TRIGGER lose_receipt").unwrap();

    let mut host = f.open();
    let result = host
        .execute_approved(&adapter, &action, "task:test")
        .unwrap();
    assert_eq!(result.schema_id.0, TOOL);
    assert_eq!(result.payload["action_version"], json!(action));
    assert_eq!(
        result.payload["result"],
        serde_json::from_str::<Value>(&saved).unwrap()
    );
    assert_eq!(result.payload["result"]["status"], "passed");
    assert_eq!(result.payload["result"]["exit_code"], 0);
    assert_eq!(f.attempts(), 1);
    host.shutdown().unwrap();
}

#[test]
fn failed_and_timed_out_checks_have_distinct_evidence_and_unknown_commands_are_rejected() {
    let f = Fixture::new();
    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    for (check, status) in [("fail", "failed"), ("slow", "timed_out_or_killed")] {
        let action = approve(&host, &candidate(&host, check, check));
        let result = host
            .execute_approved(&adapter, &action, "task:test")
            .unwrap();
        assert_eq!(result.payload["result"]["status"], status);
        assert_eq!(result.payload["result"]["check"], check);
    }
    let unknown = approve(&host, &candidate(&host, "unknown", "nonexistent"));
    assert!(
        host.execute_approved(&adapter, &unknown, "task:test")
            .is_err()
    );
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    assert_eq!(
        journal
            .query_row("SELECT count(*) FROM checks", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(f.attempts(), 0);
    drop(host);
}

#[test]
fn a_started_check_with_no_result_returns_interrupted_without_running_again() {
    let f = Fixture::new();
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();
    let action = approve(&host, &candidate(&host, "interrupted-pass", "pass"));
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    journal
        .execute_batch(
            "CREATE TRIGGER lose_result BEFORE UPDATE ON checks
        BEGIN SELECT RAISE(ABORT,'simulated adapter checkpoint rollback'); END;",
        )
        .unwrap();
    // The test removes the Core failure checkpoint too, as if the reply was
    // lost while the action was still dispatched.
    core.execute_batch("CREATE TRIGGER lose_failed_attempt BEFORE INSERT ON artifact_versions
         WHEN NEW.schema_id IN ('arcs.invocation.v1','arcs.invocation.v2') AND json_extract(NEW.artifact_json,'$.payload.status')='failed'
         BEGIN SELECT RAISE(ABORT,'lost failure response'); END;").unwrap();
    assert!(
        host.execute_approved(&adapter, &action, "task:test")
            .is_err()
    );
    assert_eq!(f.attempts(), 1);
    drop(host);
    journal.execute_batch("DROP TRIGGER lose_result").unwrap();
    core.execute_batch("DROP TRIGGER lose_failed_attempt")
        .unwrap();
    let mut host = f.open();
    let result = host
        .execute_approved(&adapter, &action, "task:test")
        .unwrap();
    assert_eq!(result.payload["result"]["status"], "interrupted");
    assert_eq!(f.attempts(), 1);
    host.shutdown().unwrap();
}

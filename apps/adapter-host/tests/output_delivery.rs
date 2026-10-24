mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SchemaId, SourceKind, SystemClock, TrustLevel};
use arcs::input::{InputMessage, InputService};
use arcs::output::OutputError;
use arcs::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use arcs_host::{Host, HostError};
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

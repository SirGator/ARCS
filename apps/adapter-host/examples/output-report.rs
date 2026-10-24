//! Explizite lokale Beispielpolicy; keine automatische Freigabe im allgemeinen Host.
use std::io::Read;

use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{
    Actor, ActorType, Artifact, SchemaId, SourceKind, SystemClock, TrustLevel, VersionId,
};
use arcs::input::{InputMessage, InputService};
use arcs::output::OutputRequest;
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use arcs_host::{Host, HostError};

const POLICY: &str = "demo.output.local-file-policy";
const SOURCE: &str = "demo.output.operator";
const REPORT: &str = "arcs.report.v1";
const RECEIPT: &str = "arcs.delivery_receipt.file.v1";

struct LocalPolicy {
    input: VersionId,
    capability: CapabilityRef,
}
impl Verifier for LocalPolicy {
    fn verify(&self, artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        let parsed = serde_json::from_value::<OutputRequest>(artifact.payload.clone());
        let passed = parsed.is_ok_and(|request| {
            artifact.schema_id.0 == "arcs.output_request.v1"
                && request.artifact_version_id == self.input
                && request.capability == self.capability
                && request.artifact_schema_id.0 == REPORT
                && request.result_schema_id.0 == RECEIPT
        });
        let verdict = if passed {
            VerificationVerdict::Pass
        } else {
            VerificationVerdict::Fail
        };
        Ok(VerificationResult {
            verdict,
            findings: vec![VerificationFinding {
                check: POLICY.into(),
                verdict,
                detail: "explicit local-file target and exact report version".into(),
            }],
        })
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: output-report <host.json> <delivery-id> <report.json>".into());
    }
    let delivery_id = args[1].to_str().ok_or("delivery ID must be UTF-8")?;
    if delivery_id.is_empty()
        || delivery_id.len() > 64
        || !delivery_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err("invalid delivery ID".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args[2])?
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err("example report exceeds 4096 bytes".into());
    }
    let payload: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut host = Host::open(std::path::Path::new(&args[0]))?;
    let mut source = AdapterRegistry::new();
    source
        .register(
            AdapterManifest {
                protocol_version: 1,
                adapter_id: AdapterId(SOURCE.into()),
                adapter_version: "1".into(),
                capabilities: vec![CapabilityDescriptor {
                    id: CapabilityId("report.input".into()),
                    contract: CapabilityContract::Input {
                        emits: vec![SchemaId(REPORT.into())],
                    },
                    required_permissions: vec![],
                }],
            },
            AdapterGrant {
                adapter_id: AdapterId(SOURCE.into()),
                producer_class: ProducerClass::System,
                enabled_capabilities: vec![CapabilityId("report.input".into())],
                granted_permissions: vec![],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: Some(SourceKind::Internal),
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                reasoning_limits_v2: None,
                reasoning_limits: None,
                data_limits: None,
            },
            host.schemas(),
            host.store(),
        )
        .map_err(HostError::Registration)?;
    let mut ids = host
        .store()
        .durable_id_generator("output-example")
        .map_err(HostError::Store)?;
    let input = InputService::new(
        &source,
        host.schemas(),
        host.store(),
        &mut ids,
        &SystemClock,
    )
    .ingest(
        &AdapterId(SOURCE.into()),
        InputMessage {
            capability_id: CapabilityId("report.input".into()),
            external_subject: Some(delivery_id.into()),
            external_reference: format!("report:{delivery_id}"),
            payload,
        },
    )
    .map_err(|source| HostError::Input {
        request: delivery_id.into(),
        source,
    })?;
    let capability = CapabilityRef::new("example.file-output", "report.write");
    let request = host.prepare_output(&capability, &input.version_id, &SchemaId(RECEIPT.into()))?;
    let policy = LocalPolicy {
        input: input.version_id.clone(),
        capability: capability.clone(),
    };
    // Auch beim Replay wird die konkrete lokale Policy erneut ausgewertet.
    if policy
        .verify(&request)
        .map_err(|error| format!("{error:?}"))?
        .verdict
        != VerificationVerdict::Pass
    {
        return Err("local output policy rejected request".into());
    }
    let approval = match host
        .store()
        .find_by_stream_key(&format!("approval:{}", request.version_id.0))
        .map_err(HostError::Store)?
    {
        Some(approval) if approval.created_by.id == POLICY => approval,
        Some(_) => return Err("existing approval belongs to another policy".into()),
        None => {
            let verification = match host
                .store()
                .find_by_stream_key(&format!("verification:{}", request.version_id.0))
                .map_err(HostError::Store)?
            {
                Some(report) => report,
                None => {
                    VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
                        .verify(&request.version_id, &policy)
                        .map_err(|error| format!("{error:?}"))?
                }
            };
            ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
                .decide(
                    &request.version_id,
                    &verification.version_id,
                    ApprovalDecision::Approved,
                    Actor {
                        actor_type: ActorType::System,
                        id: POLICY.into(),
                    },
                    "explicit local report export".into(),
                )
                .map_err(|error| format!("{error:?}"))?
        }
    };
    let receipt = host.deliver_output(
        &capability.adapter_id,
        &request.version_id,
        &approval.version_id,
    )?;
    host.shutdown()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"request_version":request.version_id,"approval_version":approval.version_id,"receipt_version":receipt.version_id,"receipt":receipt.payload})
        )?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("output report: {error}");
        std::process::exit(1);
    }
}

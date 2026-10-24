//! Nur die Beispielanwendung erteilt Freigaben; der Host-Runner bleibt allgemein.
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SystemClock};
use arcs::output::OutputRequest;
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use arcs_host::pipeline::{PipelineApproval, PipelineError, PipelineOutput, run_pipeline};
pub use arcs_host::pipeline::{PipelineConfig, PipelineReport};
use arcs_host::{Host, HostError};

const POLICY: &str = "demo.pipeline.configured-export.v1";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Diese Policy erlaubt die Ausgabe genau dieser abgeleiteten Version an das
/// konfigurierte Ziel. Sie prüft keine Wahrheit der ursprünglichen Messung.
struct ExportPolicy<'a> {
    output: &'a PipelineOutput<'a>,
}

impl Verifier for ExportPolicy<'_> {
    fn verify(
        &self,
        artifact: &Artifact,
    ) -> std::result::Result<VerificationResult, VerificationError> {
        let passed = serde_json::from_value::<OutputRequest>(artifact.payload.clone()).is_ok_and(
            |request| {
                artifact.schema_id.0 == "arcs.output_request.v1"
                    && request.artifact_version_id == self.output.transformed.version_id
                    && request.artifact_schema_id == self.output.transformed.schema_id
                    && request.capability == self.output.config.output
                    && request.result_schema_id == self.output.config.receipt_schema_id
                    && self.output.transformed.created_by.id
                        == self
                            .output
                            .config
                            .steps
                            .last()
                            .unwrap()
                            .capability
                            .adapter_id
                            .0
                    && self.output.transformed.schema_id
                        == self.output.config.steps.last().unwrap().output_schema_id
            },
        );
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
                detail: "export exact transformed version to explicitly configured target".into(),
            }],
        })
    }
}

fn application_approval(
    host: &Host,
    request: &Artifact,
    policy: &ExportPolicy<'_>,
) -> Result<Artifact> {
    // Die aktuelle Anwendungsentscheidung gilt auch vor einem Core-Cachetreffer.
    if policy
        .verify(request)
        .map_err(|error| format!("{error:?}"))?
        .verdict
        != VerificationVerdict::Pass
    {
        return Err("pipeline export policy rejected request".into());
    }
    if let Some(approval) = host
        .store()
        .find_by_stream_key(&format!("approval:{}", request.version_id.0))
        .map_err(HostError::Store)?
    {
        if approval.created_by.actor_type != ActorType::System || approval.created_by.id != POLICY {
            return Err("existing approval belongs to another policy".into());
        }
        // Der OutputService prüft Entscheidung, Verification und alle Relationen
        // vor jeder Zustellung erneut; ein Rejected wird hier nicht überstimmt.
        return Ok(approval);
    }
    let mut ids = host
        .store()
        .durable_id_generator("pipeline-policy")
        .map_err(HostError::Store)?;
    let verification = match host
        .store()
        .find_by_stream_key(&format!("verification:{}", request.version_id.0))
        .map_err(HostError::Store)?
    {
        Some(report) => report,
        None => VerificationService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
            .verify(&request.version_id, policy)
            .map_err(|error| format!("{error:?}"))?,
    };
    Ok(
        ApprovalService::new(host.schemas(), host.store(), &mut ids, &SystemClock)
            .decide(
                &request.version_id,
                &verification.version_id,
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::System,
                    id: POLICY.into(),
                },
                "explicit configured pipeline export; no claim about source truth".into(),
            )
            .map_err(|error| format!("{error:?}"))?,
    )
}

struct LocalExport;
impl PipelineApproval for LocalExport {
    fn approve(
        &mut self,
        host: &Host,
        output: &PipelineOutput<'_>,
    ) -> std::result::Result<Artifact, PipelineError> {
        application_approval(host, output.request, &ExportPolicy { output })
            .map_err(|error| PipelineError::Policy(error.to_string()))
    }
}

pub fn run(
    host: &mut Host,
    config: &PipelineConfig,
) -> std::result::Result<PipelineReport, PipelineError> {
    run_pipeline(host, config, &mut LocalExport)
}

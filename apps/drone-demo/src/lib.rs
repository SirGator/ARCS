//! Eine explizite Bewegung mit separater Verification, Freigabe und Wirkungsprüfung.
use arcs::action::ActionService;
use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{
    Actor, ActorType, Artifact, SchemaId, SequenceIdGenerator, SourceKind, SystemClock, TrustLevel,
};
use arcs::decision::proposal::{CandidateProposal, ProposalService};
use arcs::input::{InputMessage, InputService};
use arcs::outcome::{
    OutcomeError, OutcomeEvaluator, OutcomeResult, OutcomeService, OutcomeVerdict,
};
use arcs::store::{ArtifactRelations, relation_kinds};
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use arcs::world::observation::ObservationLog;
use arcs_host::Host;
use arcs_simulated_drone::{
    CANDIDATE_SCHEMA, ExecutionResult, MoveCommand, Movement, POSE_SCHEMA, Pose, QUERY_SCHEMA,
    Telemetry,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;
const OPERATOR: &str = "demo.drone.operator";
const POLICY: &str = "demo.drone.bounded-move.v1";
const CYCLE_SCHEMA: &str = "arcs.input.drone_demo.v1";
fn error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightConfig {
    pub cycle_id: String,
    pub adapter_id: AdapterId,
    pub drone_id: String,
    pub movement: Movement,
    pub approve: bool,
    pub max_auto_step_m: f64,
    pub max_auto_turn_deg: f64,
}
impl FlightConfig {
    fn validate(&self) -> Result<()> {
        if self.cycle_id.is_empty()
            || self.cycle_id.len() > 64
            || !self
                .cycle_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || self.drone_id.is_empty()
            || self.drone_id.len() > 64
            || !self.max_auto_step_m.is_finite()
            || self.max_auto_step_m <= 0.0
            || !self.max_auto_turn_deg.is_finite()
            || !(0.0..=180.0).contains(&self.max_auto_turn_deg)
        {
            return Err("invalid flight configuration".into());
        }
        Ok(())
    }
    fn capability(&self, id: &str) -> CapabilityRef {
        CapabilityRef::new(&self.adapter_id.0, id)
    }
}

struct Operator {
    registry: AdapterRegistry,
    ids: SequenceIdGenerator,
}
impl Operator {
    fn new(host: &Host) -> Result<Self> {
        let mut registry = AdapterRegistry::new();
        let capabilities = [("flight.start", CYCLE_SCHEMA), ("pose.query", QUERY_SCHEMA)]
            .into_iter()
            .map(|(id, schema)| CapabilityDescriptor {
                id: CapabilityId(id.into()),
                contract: CapabilityContract::Input {
                    emits: vec![SchemaId(schema.into())],
                },
                required_permissions: vec![],
            })
            .collect::<Vec<_>>();
        registry
            .register(
                AdapterManifest {
                    protocol_version: 1,
                    adapter_id: AdapterId(OPERATOR.into()),
                    adapter_version: "1".into(),
                    capabilities: capabilities.clone(),
                },
                AdapterGrant {
                    adapter_id: AdapterId(OPERATOR.into()),
                    producer_class: ProducerClass::System,
                    enabled_capabilities: capabilities
                        .into_iter()
                        .map(|capability| capability.id)
                        .collect(),
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
            .map_err(error)?;
        Ok(Self {
            registry,
            ids: host
                .store()
                .durable_id_generator("drone-policy")
                .map_err(error)?,
        })
    }
    fn input(
        &mut self,
        host: &Host,
        capability: &str,
        reference: &str,
        subject: &str,
        payload: Value,
    ) -> Result<Artifact> {
        InputService::new(
            &self.registry,
            host.schemas(),
            host.store(),
            &mut self.ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(OPERATOR.into()),
            InputMessage {
                capability_id: CapabilityId(capability.into()),
                external_subject: Some(subject.into()),
                external_reference: reference.into(),
                payload,
            },
        )
        .map_err(error)
    }
    fn read(&mut self, host: &mut Host, config: &FlightConfig, phase: &str) -> Result<Artifact> {
        let query = self.input(
            host,
            "pose.query",
            &format!("flight:{}:{phase}", config.cycle_id),
            &config.drone_id,
            json!({"drone_id":config.drone_id}),
        )?;
        host.request_version(
            &config.capability("pose.read"),
            &query.version_id,
            &SchemaId(POSE_SCHEMA.into()),
        )
        .map_err(error)
    }
}

struct MoveVerifier<'a> {
    config: &'a FlightConfig,
    before: &'a Artifact,
    cycle: &'a Artifact,
}
impl Verifier for MoveVerifier<'_> {
    fn verify(
        &self,
        candidate: &Artifact,
    ) -> std::result::Result<VerificationResult, VerificationError> {
        let passed = serde_json::from_value::<MoveCommand>(candidate.payload.clone())
            .ok()
            .zip(serde_json::from_value::<Telemetry>(self.before.payload.clone()).ok())
            .is_some_and(|(command, before)| {
                let movement = command.movement;
                candidate.schema_id.0 == CANDIDATE_SCHEMA
                    && self.before.schema_id.0 == POSE_SCHEMA
                    && before.drone_id == self.config.drone_id
                    && command.drone_id == before.drone_id
                    && command.expected_revision == before.revision
                    && movement == self.config.movement
                    && movement.dx_m.is_finite()
                    && movement.dy_m.is_finite()
                    && movement.dyaw_deg.is_finite()
                    && movement.dx_m.hypot(movement.dy_m) <= self.config.max_auto_step_m
                    && movement.dyaw_deg.abs() <= self.config.max_auto_turn_deg
                    && (movement.dx_m != 0.0 || movement.dy_m != 0.0 || movement.dyaw_deg != 0.0)
                    && candidate.provenance.as_ref().is_some_and(|provenance| {
                        provenance.parents
                            == [
                                self.before.version_id.0.clone(),
                                self.cycle.version_id.0.clone(),
                            ]
                    })
            });
        let verdict = if passed {
            VerificationVerdict::Pass
        } else {
            VerificationVerdict::Fail
        };
        Ok(VerificationResult {verdict,findings:vec![VerificationFinding {check:POLICY.into(),verdict,detail:"check explicit movement bounds, expected revision, target and persisted evidence; collision checks remain in adapter".into()}]})
    }
}

struct EffectCheck<'a> {
    before: &'a Telemetry,
    after: &'a Telemetry,
    observed: &'a Telemetry,
    movement: Movement,
    action: &'a Artifact,
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}
impl OutcomeEvaluator for EffectCheck<'_> {
    fn evaluate(&self, execution: &Artifact) -> std::result::Result<OutcomeResult, OutcomeError> {
        let expected = Pose {
            x_m: self.before.pose.x_m + self.movement.dx_m,
            y_m: self.before.pose.y_m + self.movement.dy_m,
            yaw_deg: (self.before.pose.yaw_deg + self.movement.dyaw_deg).rem_euclid(360.0),
        };
        let passed = serde_json::from_value::<ExecutionResult>(execution.payload.clone())
            .is_ok_and(|result| {
                result.action_version == self.action.version_id
                    && result.result == *self.after
                    && self.after == self.observed
                    && self.after.drone_id == self.before.drone_id
                    && self.before.revision.checked_add(1) == Some(self.after.revision)
                    && close(self.after.height_m, self.before.height_m)
                    && close(self.after.pose.x_m, expected.x_m)
                    && close(self.after.pose.y_m, expected.y_m)
                    && close(self.after.pose.yaw_deg, expected.yaw_deg)
            });
        Ok(OutcomeResult {verdict:if passed {OutcomeVerdict::Success}else{OutcomeVerdict::Failure},detail:"compare requested displacement with execution receipt, separate pose query and persisted telemetry observation".into()})
    }
}

pub fn run(host: &mut Host, config: &FlightConfig) -> Result<Value> {
    config.validate()?;
    let mut operator = Operator::new(host)?;
    let cycle = operator.input(
        host,
        "flight.start",
        &format!("flight:{}", config.cycle_id),
        &config.cycle_id,
        json!({"config":config,"policy":POLICY}),
    )?;
    host.poll_round().map_err(error)?;
    let before = operator.read(host, config, "before")?;
    let before_state: Telemetry = serde_json::from_value(before.payload.clone()).map_err(error)?;
    let capability = config.capability("drone.move");
    let candidate = ProposalService::new(
        host.registry(),
        host.schemas(),
        host.store(),
        &mut operator.ids,
        &SystemClock,
    )
    .propose(CandidateProposal {
        proposal_id: format!("drone:{}", cycle.version_id.0),
        rule_id: POLICY.into(),
        schema_id: SchemaId(CANDIDATE_SCHEMA.into()),
        payload: serde_json::to_value(MoveCommand {
            drone_id: config.drone_id.clone(),
            expected_revision: before_state.revision,
            movement: config.movement,
        })
        .map_err(error)?,
        evidence: vec![before.version_id.clone(), cycle.version_id.clone()],
        required_capabilities: vec![capability.clone()],
    })
    .map_err(error)?;
    let verifier = MoveVerifier {
        config,
        before: &before,
        cycle: &cycle,
    };
    let fresh = verifier.verify(&candidate).map_err(error)?;
    let verification = match host
        .store()
        .find_by_stream_key(&format!("verification:{}", candidate.version_id.0))
        .map_err(error)?
    {
        Some(report) => report,
        None => VerificationService::new(
            host.schemas(),
            host.store(),
            &mut operator.ids,
            &SystemClock,
        )
        .verify(&candidate.version_id, &verifier)
        .map_err(error)?,
    };
    let passed =
        fresh.verdict == VerificationVerdict::Pass && verification.payload["verdict"] == "pass";
    let approval = match host
        .store()
        .find_by_stream_key(&format!("approval:{}", candidate.version_id.0))
        .map_err(error)?
    {
        Some(approval) if approval.created_by.id == POLICY => approval,
        Some(_) => return Err("approval belongs to another policy".into()),
        None => ApprovalService::new(
            host.schemas(),
            host.store(),
            &mut operator.ids,
            &SystemClock,
        )
        .decide(
            &candidate.version_id,
            &verification.version_id,
            if passed && config.approve {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Rejected
            },
            Actor {
                actor_type: ActorType::System,
                id: POLICY.into(),
            },
            "explicit bounded-movement policy for simulation".into(),
        )
        .map_err(error)?,
    };
    let mut references = json!({"cycle":cycle.version_id,"before":before.version_id,"candidate":candidate.version_id,"verification":verification.version_id,"approval":approval.version_id});
    let mut status = if passed {
        "rejected"
    } else {
        "verification_failed"
    };
    let mut execution = None;
    if passed && config.approve && approval.payload["decision"] == "approved" {
        let action = ActionService::new(
            host.registry(),
            host.schemas(),
            host.store(),
            &mut operator.ids,
            &SystemClock,
        )
        .materialize(&approval.version_id, &capability)
        .map_err(error)?;
        let result = host
            .execute_approved(&config.adapter_id, &action.version_id)
            .map_err(error)?;
        references["action"] = json!(action.version_id);
        references["execution"] = json!(result.version_id);
        execution = Some((action, result));
    }
    let after = operator.read(host, config, "after")?;
    let after_state: Telemetry = serde_json::from_value(after.payload.clone()).map_err(error)?;
    host.poll_round().map_err(error)?;
    references["after"] = json!(after.version_id);
    if let Some((action, result)) = execution {
        let observed = ObservationLog::new(host.store())
            .after(None)
            .map_err(error)?
            .into_iter()
            .rev()
            .map(|recorded| recorded.into_artifact())
            .find(|artifact| {
                artifact.created_by.id == config.adapter_id.0
                    && artifact.schema_id.0 == POSE_SCHEMA
                    && artifact.tags.contains(&"capability:pose.observe".into())
                    && artifact.payload == after.payload
            })
            .ok_or("missing committed post-movement telemetry")?;
        let observed_state: Telemetry =
            serde_json::from_value(observed.payload.clone()).map_err(error)?;
        let outcome = match host
            .store()
            .find_by_stream_key(&format!("outcome:{}", result.version_id.0))
            .map_err(error)?
        {
            Some(outcome) => outcome,
            None => OutcomeService::new(
                host.schemas(),
                host.store(),
                &mut operator.ids,
                &SystemClock,
            )
            .evaluate(
                &result.version_id,
                &EffectCheck {
                    before: &before_state,
                    after: &after_state,
                    observed: &observed_state,
                    movement: config.movement,
                    action: &action,
                },
            )
            .map_err(error)?,
        };
        let relations = ArtifactRelations::new(host.store());
        for evidence in [&after.version_id, &observed.version_id] {
            if !relations
                .outgoing(&outcome.version_id)
                .map_err(error)?
                .iter()
                .any(|edge| edge.to == *evidence && edge.kind == relation_kinds::supported_by())
            {
                relations
                    .connect(
                        outcome.version_id.clone(),
                        evidence.clone(),
                        relation_kinds::supported_by(),
                    )
                    .map_err(error)?;
            }
        }
        references["observation"] = json!(observed.version_id);
        references["outcome"] = json!(outcome.version_id);
        status = if outcome.payload["verdict"] == "success" {
            "success"
        } else {
            "outcome_failed"
        };
    }
    Ok(
        json!({"cycle_id":config.cycle_id,"status":status,"before":before_state,"after":after_state,"artifacts":references,"host":host.report().map_err(error)?}),
    )
}

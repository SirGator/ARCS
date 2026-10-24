//! Deterministischer Regelkreis mit zwei externen stdio-Fähigkeiten.

use arcs_simulated_fan as fan;

use std::process::Command;
use std::time::Duration;

use arcs::action::ActionService;
use arcs::adapters::stdio::StdioAdapter;
use arcs::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{
    Actor, ActorType, Artifact, ArtifactIdGenerator, SchemaId, SchemaRegistry, SourceKind,
    SystemClock, TrustLevel,
};
use arcs::decision::proposal::{CandidateProposal, ProposalService};
use arcs::execution::ExecutionService;
use arcs::input::{InputMessage, InputService};
use arcs::outcome::{
    OutcomeError, OutcomeEvaluator, OutcomeResult, OutcomeService, OutcomeVerdict,
};
use arcs::request::RequestService;
use arcs::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use arcs::world::observation::{ObservationLog, ObservationService};
use arcs::world::{WorldReducer, WorldState};
use serde_json::{Value, json};

const HOST: &str = "demo.fan.host";
const CYCLE: &str = "arcs.input.fan_cycle.v1";

fn error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn schemas() -> Result<SchemaRegistry, String> {
    let mut schemas = SchemaRegistry::with_bundled_schemas().map_err(error)?;
    for document in fan::SCHEMAS {
        schemas.register_json(document).map_err(error)?;
    }
    Ok(schemas)
}

fn grant() -> AdapterGrant {
    let manifest = fan::manifest();
    AdapterGrant {
        adapter_id: manifest.adapter_id,
        producer_class: ProducerClass::Executor,
        enabled_capabilities: manifest.capabilities.into_iter().map(|c| c.id).collect(),
        granted_permissions: vec!["sensor.read".into(), "fan.write".into()],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
    }
}

fn register_host(
    registry: &mut AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
) -> Result<(), String> {
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(HOST.into()),
        adapter_version: "1.0.0".into(),
        capabilities: [
            ("cycle.start", CYCLE),
            ("temperature.request", fan::TEMPERATURE_REQUEST),
            ("fan.request", fan::FAN_REQUEST),
        ]
        .into_iter()
        .map(|(id, schema)| CapabilityDescriptor {
            id: CapabilityId(id.into()),
            contract: CapabilityContract::Input {
                emits: vec![SchemaId(schema.into())],
            },
            required_permissions: vec![],
        })
        .collect(),
    };
    let grant = AdapterGrant {
        adapter_id: manifest.adapter_id.clone(),
        producer_class: ProducerClass::System,
        enabled_capabilities: manifest.capabilities.iter().map(|c| c.id.clone()).collect(),
        granted_permissions: vec![],
        assigned_trust: TrustLevel::High,
        ingress_source_kind: Some(SourceKind::Internal),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
    };
    registry
        .register(manifest, grant, schemas, store)
        .map_err(error)
}

struct Host<'a> {
    registry: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
}

impl Host<'_> {
    fn input(
        &mut self,
        capability: &str,
        reference: &str,
        subject: &str,
        payload: Value,
    ) -> Result<Artifact, String> {
        InputService::new(
            self.registry,
            self.schemas,
            self.store,
            self.ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(HOST.into()),
            InputMessage {
                capability_id: CapabilityId(capability.into()),
                external_subject: Some(subject.into()),
                external_reference: reference.into(),
                payload,
            },
        )
        .map_err(error)
    }

    fn read(
        &mut self,
        adapter: &StdioAdapter,
        cycle: &str,
        device: &str,
        temperature: bool,
    ) -> Result<Artifact, String> {
        let (input_capability, capability, response) = if temperature {
            ("temperature.request", "temperature.read", fan::TEMPERATURE)
        } else {
            ("fan.request", "fan.read", fan::FAN_STATE)
        };
        let request = self.input(
            input_capability,
            &format!("cycle:{cycle}:{capability}"),
            device,
            json!({"device_id": device}),
        )?;
        RequestService::new(
            self.registry,
            self.schemas,
            self.store,
            self.ids,
            &SystemClock,
        )
        .execute(
            adapter,
            &CapabilityRef::new(fan::ADAPTER, capability),
            &request.version_id,
            &SchemaId(response.into()),
        )
        .map_err(error)
    }

    fn drain(&mut self, adapter: &mut StdioAdapter, world: &mut WorldState) -> Result<(), String> {
        let mut observations = ObservationService::new(
            self.registry,
            self.schemas,
            self.store,
            self.ids,
            &SystemClock,
        );
        while let Some(recorded) = adapter.poll(&mut observations).map_err(error)? {
            if world
                .cursor()
                .is_none_or(|cursor| recorded.cursor() > cursor)
            {
                WorldReducer::new()
                    .reduce(world, &recorded)
                    .map_err(error)?;
            }
        }
        Ok(())
    }
}

struct FanVerifier<'a> {
    profile: &'a fan::Profile,
    temperature: &'a Artifact,
    cycle: &'a Artifact,
}

impl Verifier for FanVerifier<'_> {
    fn verify(&self, candidate: &Artifact) -> Result<VerificationResult, VerificationError> {
        let command = serde_json::from_value::<fan::FanCommand>(candidate.payload.clone()).ok();
        let reading = self.temperature.payload["value"].as_f64();
        let valid = command.is_some_and(|c| c.device_id == self.profile.fan_device && c.enabled)
            && self.temperature.schema_id.0 == fan::TEMPERATURE
            && self.temperature.payload["unit"] == "Cel"
            && reading.is_some_and(|t| {
                t.is_finite()
                    && t > self.profile.activate_above_celsius
                    && t <= self.profile.max_auto_approve_celsius
            })
            && candidate.provenance.as_ref().is_some_and(|p| {
                p.parents
                    == [
                        self.temperature.version_id.0.clone(),
                        self.cycle.version_id.0.clone(),
                    ]
            });
        let verdict = if valid {
            VerificationVerdict::Pass
        } else {
            VerificationVerdict::Fail
        };
        Ok(VerificationResult {
            verdict,
            findings: vec![VerificationFinding {
                check: "demo.fan.threshold_and_evidence".into(),
                verdict,
                detail: "check target, temperature unit, configured range and persisted evidence"
                    .into(),
            }],
        })
    }
}

struct FanOutcome<'a> {
    readback: &'a Artifact,
    device: &'a str,
}

impl OutcomeEvaluator for FanOutcome<'_> {
    fn evaluate(&self, execution: &Artifact) -> Result<OutcomeResult, OutcomeError> {
        let matches = execution.payload["result"]["device_id"] == self.device
            && execution.payload["result"]["enabled"] == true
            && self.readback.payload["device_id"] == self.device
            && self.readback.payload["enabled"] == true;
        Ok(OutcomeResult {
            verdict: if matches {
                OutcomeVerdict::Success
            } else {
                OutcomeVerdict::Failure
            },
            detail: format!("simulated fan readback {}", self.readback.version_id.0),
        })
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let approve = args.last().is_none_or(|arg| arg != "--reject");
    if !approve {
        args.pop();
    }
    if !(4..=5).contains(&args.len()) {
        return Err("usage: arcs-fan-demo <adapter-executable> <core.sqlite> <adapter.sqlite> <profile.json> [cycle-id] [--reject]".into());
    }
    let cycle_id = args
        .get(4)
        .map(|s| s.to_str().ok_or("cycle ID must be UTF-8"))
        .transpose()?
        .unwrap_or("cycle-1");
    if cycle_id.is_empty()
        || cycle_id.len() > 64
        || !cycle_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("invalid cycle ID".into());
    }
    let profile = fan::Profile::load(std::path::Path::new(&args[3]))?;
    let schemas = schemas()?;
    let store = SqliteArtifactStore::open(&args[1]).map_err(error)?;
    let mut ids = store.durable_id_generator("fan-demo").map_err(error)?;
    let mut registry = AdapterRegistry::new();
    register_host(&mut registry, &schemas, &store)?;
    let mut command = Command::new(&args[0]);
    command.arg(&args[3]).arg(&args[2]);
    let mut adapter = StdioAdapter::connect(
        &mut command,
        Duration::from_secs(5),
        grant(),
        &mut registry,
        &schemas,
        &store,
    )
    .map_err(error)?;
    let mut host = Host {
        registry: &registry,
        schemas: &schemas,
        store: &store,
        ids: &mut ids,
    };
    // Gleiche Cycle-ID bedeutet gleiche Eingabe und Freigabepolicy.
    // Eine geänderte Konfiguration braucht einen neuen expliziten Zyklus.
    let cycle = host.input(
        "cycle.start",
        &format!("cycle:{cycle_id}"),
        cycle_id,
        json!({"profile": profile, "approve": approve}),
    )?;
    let mut world = WorldState::new();
    for recorded in ObservationLog::new(&store).after(None).map_err(error)? {
        WorldReducer::new()
            .reduce(&mut world, &recorded)
            .map_err(error)?;
    }
    let temperature = host.read(&adapter, cycle_id, &profile.temperature_device, true)?;
    host.drain(&mut adapter, &mut world)?;
    let value = temperature.payload["value"]
        .as_f64()
        .ok_or("missing temperature value")?;
    let mut references = json!({"cycle": cycle.version_id, "temperature": temperature.version_id});
    let mut status = "no_action";
    let mut execution = None;
    if value > profile.activate_above_celsius {
        let candidate = ProposalService::new(&registry, &schemas, &store, host.ids, &SystemClock)
            .propose(CandidateProposal {
                proposal_id: format!("fan:{}", cycle.version_id.0),
                rule_id: "demo.fan.threshold.v1".into(),
                schema_id: SchemaId(fan::CANDIDATE.into()),
                payload: serde_json::to_value(fan::FanCommand {
                    device_id: profile.fan_device.clone(),
                    enabled: true,
                })
                .map_err(error)?,
                evidence: vec![temperature.version_id.clone(), cycle.version_id.clone()],
                required_capabilities: vec![CapabilityRef::new(fan::ADAPTER, "fan.set")],
            })
            .map_err(error)?;
        let report = match store
            .find_by_stream_key(&format!("verification:{}", candidate.version_id.0))
            .map_err(error)?
        {
            Some(report) => report,
            None => VerificationService::new(&schemas, &store, host.ids, &SystemClock)
                .verify(
                    &candidate.version_id,
                    &FanVerifier {
                        profile: &profile,
                        temperature: &temperature,
                        cycle: &cycle,
                    },
                )
                .map_err(error)?,
        };
        let passed = report.payload["verdict"] == "pass";
        let decision = if passed && approve {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Rejected
        };
        let approval = match store
            .find_by_stream_key(&format!("approval:{}", candidate.version_id.0))
            .map_err(error)?
        {
            Some(approval) => approval,
            None => ApprovalService::new(&schemas, &store, host.ids, &SystemClock)
                .decide(
                    &candidate.version_id,
                    &report.version_id,
                    decision,
                    Actor {
                        actor_type: ActorType::System,
                        id: "demo.fan.simulation-policy".into(),
                    },
                    "explicit host policy for the simulated fan only".into(),
                )
                .map_err(error)?,
        };
        references["candidate"] = json!(candidate.version_id);
        references["verification"] = json!(report.version_id);
        references["approval"] = json!(approval.version_id);
        status = if passed {
            "rejected"
        } else {
            "verification_failed"
        };
        if approval.payload["decision"] == "approved" {
            let action = ActionService::new(&registry, &schemas, &store, host.ids, &SystemClock)
                .materialize(
                    &approval.version_id,
                    &CapabilityRef::new(fan::ADAPTER, "fan.set"),
                )
                .map_err(error)?;
            let result = ExecutionService::new(&registry, &schemas, &store, host.ids, &SystemClock)
                .execute(&adapter, &action.version_id)
                .map_err(error)?;
            references["action"] = json!(action.version_id);
            references["execution"] = json!(result.version_id);
            execution = Some(result);
            status = "executed";
        }
    }
    let readback = host.read(&adapter, cycle_id, &profile.fan_device, false)?;
    let readback_state: fan::FanState =
        serde_json::from_value(readback.payload.clone()).map_err(error)?;
    host.drain(&mut adapter, &mut world)?;
    references["readback"] = json!(readback.version_id);
    if let Some(execution) = execution {
        let outcome = match store
            .find_by_stream_key(&format!("outcome:{}", execution.version_id.0))
            .map_err(error)?
        {
            Some(outcome) => outcome,
            None => OutcomeService::new(&schemas, &store, host.ids, &SystemClock)
                .evaluate(
                    &execution.version_id,
                    &FanOutcome {
                        readback: &readback,
                        device: &profile.fan_device,
                    },
                )
                .map_err(error)?,
        };
        references["outcome"] = json!(outcome.version_id);
        let relations = ArtifactRelations::new(&store);
        if !relations
            .outgoing(&outcome.version_id)
            .map_err(error)?
            .iter()
            .any(|r| r.to == readback.version_id && r.kind == relation_kinds::supported_by())
        {
            relations
                .connect(
                    outcome.version_id.clone(),
                    readback.version_id.clone(),
                    relation_kinds::supported_by(),
                )
                .map_err(error)?;
        }
        if outcome.payload["verdict"] != "success" {
            status = "outcome_failed";
        }
    }
    adapter.shutdown().map_err(error)?;
    let estimates: Vec<_> = world
        .estimates()
        .map(|(_, estimate)| {
            json!({
                "entity_id": estimate.entity_id().as_str(), "schema_id": estimate.schema_id(),
                "value": estimate.value(), "evidence_version": estimate.evidence_version(),
            })
        })
        .collect();
    println!(
        "{}",
        json!({
            "cycle_id": cycle_id, "status": status, "temperature": temperature.payload,
        "fan_state": readback_state, "world": estimates, "world_revision": world.revision().get(),
            "stored_artifacts": store.len().map_err(error)?, "artifacts": references,
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fan demo: {error}");
        std::process::exit(1);
    }
}

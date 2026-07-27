//! End-to-End-Nachweis der domänenneutralen Agent-Cycle-Fassade.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    ArtifactIdGenerator, BoundarySubmission, CapabilityContract, CapabilityDescriptor,
    CapabilityId, Clock, ContextSelection, DataAdapter, DataInvocation, DataResponse,
    GeneratedArtifactIds, InternalArtifactSubmission, OutputAdapter, OutputInvocation,
    OutputResponse, ProducerClass, ProposalSubmission, ReasoningAdapter, ReasoningBudget,
    ReasoningInvocation, ReasoningLimits, ReasoningResponse, ReasoningTrace,
};
use crate::core::{ArtifactId, SchemaRegistry, SourceKind, SubjectId, TrustLevel};
use crate::store::{ArtifactRelation, ArtifactRelations, SqliteArtifactStore, relation_kinds};

const CPU_SCHEMA_ID: &str = "arcs.observation.server_cpu.v1";
const INPUT_SCHEMA_ID: &str = "arcs.input.server_request.v1";
const DATA_REQUEST_SCHEMA_ID: &str = "arcs.data_request.server_processes.v1";
const PROCESSES_SCHEMA_ID: &str = "arcs.observation.server_processes.v1";
const CANDIDATE_SCHEMA_ID: &str = "arcs.response_candidate.server_diagnosis.v1";
const RESULT_SCHEMA_ID: &str = "arcs.result.server_delivery.v1";

// Diese Verträge gehören zur Server-Spezialisierung des Tests. Der
// produktive AgentCycle oberhalb kennt keinen einzigen dieser Feldnamen.
const CPU_SCHEMA: &str = r#"{
        "$id": "arcs.observation.server_cpu.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["utilization"],
        "properties": {
            "utilization": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        },
        "additionalProperties": false
    }"#;

const INPUT_SCHEMA: &str = r#"{
        "$id": "arcs.input.server_request.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["text"],
        "properties": {
            "text": {"type": "string", "minLength": 1}
        },
        "additionalProperties": false
    }"#;

const DATA_REQUEST_SCHEMA: &str = r#"{
        "$id": "arcs.data_request.server_processes.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["query"],
        "properties": {
            "query": {"type": "string", "enum": ["processes"]}
        },
        "additionalProperties": false
    }"#;

const PROCESSES_SCHEMA: &str = r#"{
        "$id": "arcs.observation.server_processes.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["processes"],
        "properties": {
            "processes": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "required": ["name", "cpu"],
                    "properties": {
                        "name": {"type": "string", "minLength": 1},
                        "cpu": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0
                        }
                    },
                    "additionalProperties": false
                }
            }
        },
        "additionalProperties": false
    }"#;

const CANDIDATE_SCHEMA: &str = r#"{
        "$id": "arcs.response_candidate.server_diagnosis.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["message"],
        "properties": {
            "message": {"type": "string", "minLength": 1}
        },
        "additionalProperties": false
    }"#;

const RESULT_SCHEMA: &str = r#"{
        "$id": "arcs.result.server_delivery.v1",
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["delivered", "channel"],
        "properties": {
            "delivered": {"type": "boolean"},
            "channel": {"type": "string", "minLength": 1}
        },
        "additionalProperties": false
    }"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-27T12:00:00Z".into()
    }
}

struct SequenceIds {
    next: u64,
}

impl ArtifactIdGenerator for SequenceIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let next = self.next;
        self.next += 1;
        let artifact_id = ArtifactId(format!("cycle-{artifact_type}-{next}"));
        GeneratedArtifactIds {
            version_id: VersionId(format!("{}-v1", artifact_id.0)),
            artifact_id,
        }
    }
}

struct ProcessDataAdapter {
    manifest: AdapterManifest,
    invocations: Arc<Mutex<Vec<DataInvocation>>>,
}

impl DataAdapter for ProcessDataAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn fetch(&self, request: &DataInvocation) -> Result<DataResponse, AdapterCallError> {
        self.invocations.lock().unwrap().push(request.clone());
        Ok(DataResponse {
            invocation_id: request.invocation_id.clone(),
            external_reference: "procfs://server-1/processes".into(),
            payload: json!({
                "processes": [
                    {"name": "backup", "cpu": 0.74},
                    {"name": "api", "cpu": 0.12}
                ]
            }),
        })
    }
}

struct ServerReasoner {
    manifest: AdapterManifest,
    calls: Arc<AtomicUsize>,
    invocations: Arc<Mutex<Vec<ReasoningInvocation>>>,
}

impl ReasoningAdapter for ServerReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.invocations.lock().unwrap().push(request.clone());
        Ok(ReasoningResponse {
            request_id: request.request_id.clone(),
            candidates: vec![ProposalSubmission {
                schema_id: request.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: request
                    .context
                    .iter()
                    .map(|item| item.version_id.clone())
                    .collect(),
                payload: json!({
                    "message": "Der Backup-Prozess verursacht die hohe CPU-Last."
                }),
            }],
            trace: ReasoningTrace {
                model_name: "server-reasoner-mock".into(),
                prompt_hash: "prompt-sha256".into(),
                raw_output_hash: "output-sha256".into(),
                temperature: 0.0,
            },
        })
    }
}

struct ChatOutputAdapter {
    manifest: AdapterManifest,
    invocations: Arc<Mutex<Vec<OutputInvocation>>>,
}

impl OutputAdapter for ChatOutputAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn deliver(&self, request: &OutputInvocation) -> Result<OutputResponse, AdapterCallError> {
        self.invocations.lock().unwrap().push(request.clone());
        Ok(OutputResponse {
            invocation_id: request.invocation_id.clone(),
            external_reference: "chat://conversation-1/message-2".into(),
            result_payload: json!({
                "delivered": true,
                "channel": "chat"
            }),
        })
    }
}

fn observe_manifest(
    adapter_id: &str,
    capability_id: &str,
    emitted_schema: &str,
    permission: &str,
) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(adapter_id.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(capability_id.into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId(emitted_schema.into())],
            },
            required_permissions: vec![permission.into()],
        }],
    }
}

fn boundary_grant(
    adapter_id: &str,
    capability_id: &str,
    permission: &str,
    source_kind: SourceKind,
    trust: TrustLevel,
) -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(adapter_id.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId(capability_id.into())],
        granted_permissions: vec![permission.into()],
        assigned_trust: trust,
        observation_source_kind: Some(source_kind),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

fn data_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("server.process-data".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("server.processes.fetch".into()),
            contract: CapabilityContract::Data {
                accepts: vec![SchemaId(DATA_REQUEST_SCHEMA_ID.into())],
                emits: vec![SchemaId(PROCESSES_SCHEMA_ID.into())],
            },
            required_permissions: vec!["server.processes.read".into()],
        }],
    }
}

fn data_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("server.process-data".into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("server.processes.fetch".into())],
        granted_permissions: vec!["server.processes.read".into()],
        assigned_trust: TrustLevel::High,
        observation_source_kind: Some(SourceKind::Api),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

fn reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.server".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("reasoning.diagnose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn reasoning_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("reasoning.server".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("reasoning.diagnose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        observation_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 4,
            max_context_bytes: 8192,
            max_output_tokens: 256,
            max_output_bytes: 4096,
            max_candidates: 2,
        }),
    }
}

fn output_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("output.chat".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("chat.deliver".into()),
            contract: CapabilityContract::Output {
                accepts: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
                emits: vec![SchemaId(RESULT_SCHEMA_ID.into())],
                idempotent: true,
            },
            required_permissions: vec!["chat.write".into()],
        }],
    }
}

fn output_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("output.chat".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId("chat.deliver".into())],
        granted_permissions: vec!["chat.write".into()],
        assigned_trust: TrustLevel::High,
        observation_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

fn reasoning_request(request_id: &str, context: Vec<ContextSelection>) -> ReasoningRequest {
    ReasoningRequest {
        request_id: request_id.into(),
        reasoning_capability: CapabilityRef::new("reasoning.server", "reasoning.diagnose"),
        objective: "Erkläre die aktuelle hohe CPU-Last.".into(),
        context,
        target_schema_id: SchemaId(CANDIDATE_SCHEMA_ID.into()),
        allowed_capabilities: vec![],
        constraints: json!({"language": "de"}),
        budget: ReasoningBudget {
            max_context_items: 4,
            max_context_bytes: 8192,
            max_output_tokens: 256,
            max_output_bytes: 4096,
            max_candidates: 1,
        },
    }
}

#[test]
fn adapters_specialize_a_persistent_cycle_without_polluting_the_core() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let reasoning_calls = Arc::new(AtomicUsize::new(0));
    let reasoning_invocations = Arc::new(Mutex::new(Vec::new()));
    let data_invocations = Arc::new(Mutex::new(Vec::new()));
    let output_invocations = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(SequenceIds { next: 1 }),
    );

    // Nur die Adapterinstallation kennt Server, CPU, Prozesse und Chat.
    let cpu_session = gateway
        .register_adapter(
            observe_manifest(
                "observation.server",
                "server.cpu.observe",
                CPU_SCHEMA_ID,
                "server.cpu.read",
            ),
            boundary_grant(
                "observation.server",
                "server.cpu.observe",
                "server.cpu.read",
                SourceKind::Sensor,
                TrustLevel::High,
            ),
            &[
                CPU_SCHEMA,
                INPUT_SCHEMA,
                DATA_REQUEST_SCHEMA,
                PROCESSES_SCHEMA,
                CANDIDATE_SCHEMA,
                RESULT_SCHEMA,
            ],
        )
        .unwrap();
    let input_session = gateway
        .register_adapter(
            observe_manifest(
                "input.chat",
                "chat.input.observe",
                INPUT_SCHEMA_ID,
                "chat.read",
            ),
            boundary_grant(
                "input.chat",
                "chat.input.observe",
                "chat.read",
                SourceKind::Chat,
                TrustLevel::High,
            ),
            &[],
        )
        .unwrap();
    gateway
        .register_data_adapter(
            Box::new(ProcessDataAdapter {
                manifest: data_manifest(),
                invocations: Arc::clone(&data_invocations),
            }),
            data_grant(),
            &[],
        )
        .unwrap();
    gateway
        .register_reasoning_adapter(
            Box::new(ServerReasoner {
                manifest: reasoning_manifest(),
                calls: Arc::clone(&reasoning_calls),
                invocations: Arc::clone(&reasoning_invocations),
            }),
            reasoning_grant(),
            &[],
        )
        .unwrap();
    gateway
        .register_output_adapter(
            Box::new(ChatOutputAdapter {
                manifest: output_manifest(),
                invocations: Arc::clone(&output_invocations),
            }),
            output_grant(),
            &[],
        )
        .unwrap();

    // R1 ist ein bekanntes, Core-seitig gespeichertes Request-Template.
    let data_request = gateway
        .record_internal(InternalArtifactSubmission {
            schema_id: SchemaId(DATA_REQUEST_SCHEMA_ID.into()),
            subject: Some(SubjectId("server-1/processes".into())),
            stream_key: "server-1:diagnosis".into(),
            internal_reference: "known-route:fetch-processes".into(),
            tags: vec!["purpose:complete_context".into()],
            payload: json!({"query": "processes"}),
            parent_versions: vec![],
        })
        .unwrap();

    // Derselbe Subject-Slot erhält zwei unveränderliche Versionen. Nur
    // der Current-Zeiger wechselt; die alte CPU-Sicht bleibt abrufbar.
    let old_cpu = gateway
        .submit_boundary(
            &cpu_session,
            BoundarySubmission {
                capability_id: CapabilityId("server.cpu.observe".into()),
                schema_id: SchemaId(CPU_SCHEMA_ID.into()),
                subject: SubjectId("server-1/cpu".into()),
                external_reference: "sensor://server-1/cpu/1".into(),
                payload: json!({"utilization": 0.51}),
            },
        )
        .unwrap();
    let current_cpu = gateway
        .submit_boundary(
            &cpu_session,
            BoundarySubmission {
                capability_id: CapabilityId("server.cpu.observe".into()),
                schema_id: SchemaId(CPU_SCHEMA_ID.into()),
                subject: SubjectId("server-1/cpu".into()),
                external_reference: "sensor://server-1/cpu/2".into(),
                payload: json!({"utilization": 0.94}),
            },
        )
        .unwrap();
    let input = gateway
        .submit_boundary(
            &input_session,
            BoundarySubmission {
                capability_id: CapabilityId("chat.input.observe".into()),
                schema_id: SchemaId(INPUT_SCHEMA_ID.into()),
                subject: SubjectId("conversation-1/request".into()),
                external_reference: "chat://conversation-1/message-1".into(),
                payload: json!({"text": "Warum ist der Server so langsam?"}),
            },
        )
        .unwrap();

    let network = ArtifactNetwork::new(&store);
    network
        .connect(
            input.version_id.clone(),
            data_request.version_id.clone(),
            0.7,
        )
        .unwrap();
    network
        .connect(
            current_cpu.version_id.clone(),
            data_request.version_id.clone(),
            0.5,
        )
        .unwrap();

    let mut cycle = AgentCycle::new(&mut gateway);
    let input_only = [ActiveSource {
        version_id: input.version_id.clone(),
        activation: 0.6,
    }];
    let combined = [
        ActiveSource {
            version_id: input.version_id.clone(),
            activation: 0.6,
        },
        ActiveSource {
            version_id: current_cpu.version_id.clone(),
            activation: 0.8,
        },
    ];

    assert!(
        cycle
            .evaluate_network(&input_only, 0.75)
            .unwrap()
            .is_empty(),
        "der Nutzerinput allein darf R1 nicht aktivieren"
    );
    let jointly_activated = cycle.evaluate_network(&combined, 0.75).unwrap();
    assert_eq!(jointly_activated.len(), 1);
    assert_eq!(
        jointly_activated[0].artifact.version_id,
        data_request.version_id
    );
    assert!((jointly_activated[0].activation - 0.82).abs() < 0.000_001);

    // Auch der hybride Einstieg bleibt auf dem bekannten Fast Path. Der
    // mitgegebene Fallback ist hier bewusst nur eine ungenutzte Option.
    let known_request = cycle
        .resolve_with_fallback(
            &combined,
            0.75,
            &KnownRoutePolicy {
                eligible_schema_ids: vec![SchemaId(DATA_REQUEST_SCHEMA_ID.into())],
                minimum_trust: TrustLevel::High,
            },
            reasoning_request("must-not-run", vec![]),
        )
        .unwrap();
    let activated_request = match known_request {
        RouteResolution::KnownCandidates(mut candidates) => {
            assert_eq!(candidates.len(), 1);
            candidates.remove(0)
        }
        other => panic!("bekannter Request muss Fast Path bleiben: {other:?}"),
    };
    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 0);

    let processes = cycle
        .acquire_data(
            &CapabilityRef::new("server.process-data", "server.processes.fetch"),
            &activated_request.artifact.version_id,
            &SchemaId(PROCESSES_SCHEMA_ID.into()),
        )
        .unwrap();
    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 0);
    assert_eq!(data_invocations.lock().unwrap().len(), 1);

    // Die zweite Auswertung kennt noch keine Response-Kante. Erst jetzt
    // bekommt das Modell genau I1, die aktuelle CPU-Sicht und O2.
    let response_sources = [
        combined[0].clone(),
        combined[1].clone(),
        ActiveSource {
            version_id: processes.version_id.clone(),
            activation: 1.0,
        },
    ];
    let curated_context = vec![
        ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["text".into()],
        },
        ContextSelection {
            version_id: current_cpu.version_id.clone(),
            payload_fields: vec!["utilization".into()],
        },
        ContextSelection {
            version_id: processes.version_id.clone(),
            payload_fields: vec!["processes".into()],
        },
    ];
    let resolution = cycle
        .resolve_with_fallback(
            &response_sources,
            0.75,
            &KnownRoutePolicy {
                eligible_schema_ids: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
                minimum_trust: TrustLevel::Low,
            },
            reasoning_request("server-diagnosis-1", curated_context),
        )
        .unwrap();
    let mut proposals = match resolution {
        RouteResolution::ReasonedCandidates(proposals) => proposals,
        other => panic!("unbekannte Response braucht Reasoning: {other:?}"),
    };
    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 1);
    assert_eq!(proposals.len(), 1);

    let recorded_reasoning = reasoning_invocations.lock().unwrap();
    assert_eq!(recorded_reasoning.len(), 1);
    assert_eq!(
        recorded_reasoning[0]
            .context
            .iter()
            .map(|item| item.version_id.clone())
            .collect::<Vec<_>>(),
        vec![
            input.version_id.clone(),
            current_cpu.version_id.clone(),
            processes.version_id.clone(),
        ]
    );
    drop(recorded_reasoning);

    let proposal = proposals.remove(0);
    let reasoning_request_version = proposal.reasoning_request_version().clone();
    let candidate = cycle.commit_proposal(proposal).unwrap();
    let result = cycle
        .deliver_output(
            &CapabilityRef::new("output.chat", "chat.deliver"),
            &candidate.version_id,
            &SchemaId(RESULT_SCHEMA_ID.into()),
        )
        .unwrap();

    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 1);
    let delivered = output_invocations.lock().unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].artifact_version_id, candidate.version_id);
    assert_eq!(
        delivered[0].payload,
        json!({"message": "Der Backup-Prozess verursacht die hohe CPU-Last."})
    );
    drop(delivered);

    // Historie und Current-State sind getrennte Konzepte.
    assert_eq!(
        store
            .current(
                &SubjectId("server-1/cpu".into()),
                &SchemaId(CPU_SCHEMA_ID.into()),
            )
            .unwrap()
            .unwrap()
            .version_id,
        current_cpu.version_id
    );
    assert_eq!(
        store.get(&old_cpu.version_id).unwrap().unwrap().payload,
        json!({"utilization": 0.51})
    );
    assert_eq!(
        store
            .current(
                &SubjectId("server-1/processes".into()),
                &SchemaId(PROCESSES_SCHEMA_ID.into()),
            )
            .unwrap()
            .unwrap()
            .version_id,
        processes.version_id
    );

    let process_relations = ArtifactRelations::new(&store)
        .outgoing(&processes.version_id)
        .unwrap();
    for kind in [relation_kinds::fulfills(), relation_kinds::caused_by()] {
        assert!(process_relations.contains(&ArtifactRelation {
            from: processes.version_id.clone(),
            to: data_request.version_id.clone(),
            kind,
        }));
    }

    let candidate_relations = ArtifactRelations::new(&store)
        .outgoing(&candidate.version_id)
        .unwrap();
    for context in [&input, &current_cpu, &processes] {
        assert!(candidate_relations.contains(&ArtifactRelation {
            from: candidate.version_id.clone(),
            to: context.version_id.clone(),
            kind: relation_kinds::supported_by(),
        }));
    }
    assert!(candidate_relations.contains(&ArtifactRelation {
        from: candidate.version_id.clone(),
        to: reasoning_request_version,
        kind: relation_kinds::generated_by(),
    }));

    assert!(
        ArtifactRelations::new(&store)
            .outgoing(&result.version_id)
            .unwrap()
            .contains(&ArtifactRelation {
                from: result.version_id,
                to: candidate.version_id,
                kind: relation_kinds::result_of(),
            })
    );
    assert_eq!(store.len().unwrap(), 8);
}

use super::*;
use crate::adapters::data::{DataChunk, DataLimits, DataRead};
use crate::adapters::*;
use crate::core::*;
use crate::reasoning::general::{GeneralReasoningAdapter, GeneralReasoningService};
use crate::reasoning::*;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Endpoint {
    manifest: AdapterManifest,
    reads: AtomicUsize,
    reasons: AtomicUsize,
    fail_next_read: AtomicBool,
}
impl data::DataAdapter for Endpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn read_chunk(&self, request: &DataRead) -> Result<DataChunk, AdapterCallError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if self.fail_next_read.swap(false, Ordering::Relaxed) {
            return Err(AdapterCallError::Timeout);
        }
        Ok(DataChunk {
            reference_id: request.reference.id.clone(),
            offset: request.offset,
            bytes: (0..request.expected_bytes().unwrap())
                .map(|i| ((request.offset + i as u64) % 251) as u8)
                .collect(),
        })
    }
}
impl GeneralReasoningAdapter for Endpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn propose_v2(
        &self,
        _: &ReasoningInvocationV2,
    ) -> Result<ReasoningResponseV2, AdapterCallError> {
        panic!("must use bound data callback")
    }
    fn propose_with_data(
        &self,
        invocation: &ReasoningInvocationV2,
        inputs: &[ReceivedInputData],
    ) -> Result<ReasoningResponseV2, AdapterCallError> {
        self.reasons.fetch_add(1, Ordering::Relaxed);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].bytes.len(), 100_001);
        assert!(
            inputs[0]
                .bytes
                .iter()
                .enumerate()
                .all(|(i, b)| *b == (i % 251) as u8)
        );
        Ok(ReasoningResponseV2 {
            invocation_id: invocation.invocation_id.clone(),
            request_id: invocation.request_id.clone(),
            status: ReasoningStatus::Solved,
            candidates: vec![ProposalSubmission {
                schema_id: invocation.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: vec![VersionId("binary-v1".into())],
                payload: json!({"bytes":inputs[0].bytes.len()}),
            }],
            trace: ReasoningTraceV2 {
                method: ReasoningMethod::Rules,
                implementation: "count-bytes".into(),
                version: "1".into(),
                iterations: Some(1),
                output_tokens: None,
                details: json!({}),
            },
        })
    }
}
struct Fixture {
    schemas: SchemaRegistry,
    store: SqliteArtifactStore,
    source: Endpoint,
    receiver: Endpoint,
    source_grant: AdapterGrant,
    receiver_grant: AdapterGrant,
    context: Vec<ReasoningContextItem>,
}
impl Fixture {
    fn new() -> Self {
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        schemas.register_json(r#"{"$id":"arcs.data.binary.v1","type":"object","required":["object"],"properties":{"object":{"type":"object"}}}"#).unwrap();
        schemas.register_json(r#"{"$id":"arcs.result_candidate.binary.v1","type":"object","required":["bytes"],"properties":{"bytes":{"type":"integer"}},"additionalProperties":false}"#).unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let make = |id: &str, contract| Endpoint {
            manifest: AdapterManifest {
                protocol_version: 1,
                adapter_id: AdapterId(id.into()),
                adapter_version: "1".into(),
                capabilities: vec![CapabilityDescriptor {
                    id: CapabilityId("run".into()),
                    contract,
                    required_permissions: vec![],
                }],
            },
            reads: AtomicUsize::new(0),
            reasons: AtomicUsize::new(0),
            fail_next_read: AtomicBool::new(false),
        };
        let source = make(
            "source",
            CapabilityContract::Observe {
                emits: vec![SchemaId("arcs.data.binary.v1".into())],
            },
        );
        let receiver = make(
            "receiver",
            CapabilityContract::Reason {
                emits: vec![SchemaId("arcs.result_candidate.binary.v1".into())],
            },
        );
        let grant = |id: &str| AdapterGrant {
            adapter_id: AdapterId(id.into()),
            producer_class: ProducerClass::Adapter,
            enabled_capabilities: vec![CapabilityId("run".into())],
            granted_permissions: vec![],
            assigned_trust: TrustLevel::Medium,
            ingress_source_kind: None,
            max_payload_bytes: 4096,
            max_external_reference_bytes: 256,
            data_limits: None,
            input_data_limits: None,
            reasoning_limits: None,
            reasoning_limits_v2: None,
        };
        let mut source_grant = grant("source");
        source_grant.ingress_source_kind = Some(SourceKind::File);
        source_grant.granted_permissions.push("data.read".into());
        source_grant.data_limits = Some(DataLimits {
            max_object_bytes: 200_000,
            max_chunk_bytes: 4096,
        });
        let mut receiver_grant = grant("receiver");
        receiver_grant
            .granted_permissions
            .push("data.receive".into());
        receiver_grant.input_data_limits = Some(InputDataLimits {
            max_total_bytes: 200_000,
            max_objects: 2,
        });
        receiver_grant.reasoning_limits_v2 = Some(ReasoningLimitsV2 {
            methods: vec![ReasoningMethod::Rules],
            resources: budget(),
        });
        let mut artifact = Artifact::new(
            "binary",
            "binary-v1",
            "data",
            "arcs.data.binary.v1",
            "2026-10-24T12:00:00Z",
            Actor {
                actor_type: ActorType::Adapter,
                id: "source".into(),
            },
            Source {
                kind: SourceKind::File,
                reference: "snapshot-1".into(),
            },
            Trust {
                level: TrustLevel::Low,
                source_class: SourceClass::External,
            },
            "binary",
            json!({"object":{"id":"object-1","media_type":"application/octet-stream","size_bytes":100001}}),
        );
        artifact.tags.push("capability:run".into());
        store.append(&artifact, &schemas).unwrap();
        let context = vec![ReasoningContextItem {
            version_id: artifact.version_id,
            schema_id: artifact.schema_id,
            artifact_type: artifact.artifact_type,
            trust_level: artifact.trust.level,
            payload: artifact.payload,
        }];
        Self {
            schemas,
            store,
            source,
            receiver,
            source_grant,
            receiver_grant,
            context,
        }
    }
    fn registry(&self) -> AdapterRegistry {
        let mut registry = AdapterRegistry::new();
        registry
            .register(
                self.source.manifest.clone(),
                self.source_grant.clone(),
                &self.schemas,
                &self.store,
            )
            .unwrap();
        registry
            .register(
                self.receiver.manifest.clone(),
                self.receiver_grant.clone(),
                &self.schemas,
                &self.store,
            )
            .unwrap();
        registry
    }
    fn prepare(
        &self,
        context: &[ReasoningContextItem],
        selections: &[InputDataSelection],
    ) -> Result<Vec<InputDataDescriptor>, InputDataError> {
        InputDataService::new(&self.registry(), &self.schemas, &self.store).prepare(
            &CapabilityRef::new("receiver", "run"),
            context,
            selections,
        )
    }
}
fn selection() -> InputDataSelection {
    InputDataSelection {
        name: "object".into(),
        source: CapabilityRef::new("source", "run"),
        source_version_id: VersionId("binary-v1".into()),
        reference_pointer: "/object".into(),
    }
}
fn budget() -> ReasoningResources {
    ReasoningResources {
        max_context_items: 1,
        max_context_bytes: 8192,
        max_output_bytes: 4096,
        max_candidates: 1,
        max_runtime_ms: 5000,
        max_iterations: Some(10),
        max_output_tokens: None,
    }
}
fn request() -> ReasoningRequestV2 {
    ReasoningRequestV2 {
        request_id: "read-1".into(),
        capability: CapabilityRef::new("receiver", "run"),
        method: ReasoningMethod::Rules,
        objective: "count selected binary bytes".into(),
        context: vec![ContextSelection {
            version_id: VersionId("binary-v1".into()),
            payload_fields: vec!["object".into()],
        }],
        target_schema_id: SchemaId("arcs.result_candidate.binary.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: budget(),
    }
}

#[test]
fn preflight_requires_selected_reference_exact_provenance_and_both_grants() {
    let mut fixture = Fixture::new();
    assert!(fixture.prepare(&fixture.context, &[selection()]).is_ok());
    assert!(fixture.prepare(&[], &[selection()]).is_err());
    let mut context = fixture.context.clone();
    context[0].payload = json!({});
    assert!(fixture.prepare(&context, &[selection()]).is_err());
    context[0].payload =
        json!({"object":{"id":"object-2","media_type":"application/octet-stream","size_bytes":1}});
    assert!(fixture.prepare(&context, &[selection()]).is_err());
    let mut wrong = selection();
    wrong.source = CapabilityRef::new("receiver", "run");
    assert!(fixture.prepare(&fixture.context, &[wrong]).is_err());
    fixture.receiver_grant.input_data_limits = None;
    assert!(fixture.prepare(&fixture.context, &[selection()]).is_err());
    fixture.receiver_grant.input_data_limits = Some(InputDataLimits {
        max_total_bytes: 200_000,
        max_objects: 2,
    });
    fixture.source_grant.data_limits = None;
    fixture.source_grant.granted_permissions.clear();
    assert!(fixture.prepare(&fixture.context, &[selection()]).is_err());
    assert_eq!(fixture.source.reads.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.receiver.reasons.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.store.len().unwrap(), 1);
}
#[test]
fn aggregate_limits_and_duplicate_names_are_checked_before_reads() {
    let mut fixture = Fixture::new();
    assert!(
        fixture
            .prepare(&fixture.context, &[selection(), selection()])
            .is_err()
    );
    let mut other = selection();
    other.name = "other".into();
    assert!(
        fixture
            .prepare(&fixture.context, &[selection(), other.clone()])
            .is_err()
    );
    fixture
        .receiver_grant
        .input_data_limits
        .as_mut()
        .unwrap()
        .max_total_bytes = 300_000;
    fixture
        .receiver_grant
        .input_data_limits
        .as_mut()
        .unwrap()
        .max_objects = 1;
    assert!(
        fixture
            .prepare(&fixture.context, &[selection(), other])
            .is_err()
    );
    fixture
        .source_grant
        .data_limits
        .as_mut()
        .unwrap()
        .max_object_bytes = 100_000;
    assert!(fixture.prepare(&fixture.context, &[selection()]).is_err());
    assert_eq!(fixture.source.reads.load(Ordering::Relaxed), 0);
}
#[test]
fn bounded_data_reaches_reasoner_once_cache_keeps_trust_and_rechecks_revocation() {
    let mut fixture = Fixture::new();
    let registry = fixture.registry();
    let mut ids = fixture.store.durable_id_generator("binary-test").unwrap();
    let mut service = GeneralReasoningService {
        registry: &registry,
        schemas: &fixture.schemas,
        store: &fixture.store,
        ids: &mut ids,
        clock: &SystemClock,
    };
    let result = service
        .reason_with_data(
            &fixture.receiver,
            request(),
            &[selection()],
            &[&fixture.source],
        )
        .unwrap();
    assert_eq!(result.candidates[0].trust.level, TrustLevel::Low);
    let count = fixture.store.len().unwrap();
    let replay = service
        .reason_with_data(
            &fixture.receiver,
            request(),
            &[selection()],
            &[&fixture.source],
        )
        .unwrap();
    assert_eq!(result.candidates, replay.candidates);
    assert_eq!(fixture.store.len().unwrap(), count);
    assert_eq!(fixture.source.reads.load(Ordering::Relaxed), 25);
    assert_eq!(fixture.receiver.reasons.load(Ordering::Relaxed), 1);
    let mut changed = selection();
    changed.name = "other".into();
    assert!(
        service
            .reason_with_data(&fixture.receiver, request(), &[changed], &[&fixture.source])
            .is_err()
    );
    fixture.receiver_grant.input_data_limits = None;
    let registry = fixture.registry();
    let mut service = GeneralReasoningService {
        registry: &registry,
        schemas: &fixture.schemas,
        store: &fixture.store,
        ids: &mut ids,
        clock: &SystemClock,
    };
    assert!(
        service
            .reason_with_data(
                &fixture.receiver,
                request(),
                &[selection()],
                &[&fixture.source]
            )
            .is_err()
    );
    assert_eq!(fixture.store.len().unwrap(), count);
    assert_eq!(fixture.receiver.reasons.load(Ordering::Relaxed), 1);
}
#[test]
fn receiver_limits_require_receive_permission_and_a_reason_role() {
    let mut fixture = Fixture::new();
    fixture.receiver_grant.granted_permissions.clear();
    assert!(
        AdapterRegistry::new()
            .validate_registration(
                &fixture.receiver.manifest,
                &fixture.receiver_grant,
                &fixture.schemas
            )
            .is_err()
    );
    fixture.source_grant.input_data_limits = Some(InputDataLimits {
        max_total_bytes: 1,
        max_objects: 1,
    });
    fixture
        .source_grant
        .granted_permissions
        .push("data.receive".into());
    assert!(
        AdapterRegistry::new()
            .validate_registration(
                &fixture.source.manifest,
                &fixture.source_grant,
                &fixture.schemas
            )
            .is_err()
    );
}

#[test]
fn transient_source_failure_retries_the_same_invocation_before_any_backend_call() {
    let fixture = Fixture::new();
    let registry = fixture.registry();
    let mut ids = fixture.store.durable_id_generator("retry-test").unwrap();
    let mut service = GeneralReasoningService {
        registry: &registry,
        schemas: &fixture.schemas,
        store: &fixture.store,
        ids: &mut ids,
        clock: &SystemClock,
    };
    fixture.source.fail_next_read.store(true, Ordering::Relaxed);
    assert!(
        service
            .reason_with_data(
                &fixture.receiver,
                request(),
                &[selection()],
                &[&fixture.source]
            )
            .is_err()
    );
    assert_eq!(fixture.receiver.reasons.load(Ordering::Relaxed), 0);
    let result = service
        .reason_with_data(
            &fixture.receiver,
            request(),
            &[selection()],
            &[&fixture.source],
        )
        .unwrap();
    assert_eq!(result.status, ReasoningStatus::Solved);
    assert_eq!(fixture.receiver.reasons.load(Ordering::Relaxed), 1);
}

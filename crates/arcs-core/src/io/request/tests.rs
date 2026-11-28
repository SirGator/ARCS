use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::service::{capability_name, invocation_id};
use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel,
    VersionId,
};
use crate::runtime::{InvocationKind, InvocationService, InvocationSpec, InvocationStatus};
use crate::store::SqliteArtifactStore;

const REQUEST_SCHEMA: &str = r#"{
    "$id": "arcs.request.slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["metric"],
    "properties": {"metric": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESPONSE_SCHEMA: &str = r#"{
    "$id": "arcs.observation.request_slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["value"],
    "properties": {"value": {"type": "number"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-01T12:00:00Z".into()
    }
}

struct TestIds;

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-response")),
            version_id: VersionId(format!("{artifact_type}-response-v1")),
        }
    }
}

struct RecordingEndpoint {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<RequestInvocation>>>,
    mismatched_correlation: bool,
}

impl RequestAdapter for RecordingEndpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn fetch(
        &self,
        request: &RequestInvocation,
    ) -> Result<RequestResponse, crate::adapters::AdapterCallError> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(RequestResponse {
            invocation_id: if self.mismatched_correlation {
                "foreign-invocation".into()
            } else {
                request.invocation_id.clone()
            },
            external_reference: "https://metrics.example/server-01/cpu".into(),
            payload: json!({"value": 0.92}),
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(REQUEST_SCHEMA).unwrap();
    schemas.register_json(RESPONSE_SCHEMA).unwrap();
    schemas
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("metrics.request-test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            description: None,
            id: CapabilityId("metrics.fetch".into()),
            contract: CapabilityContract::Request {
                accepts: vec![SchemaId("arcs.request.slice_test.v1".into())],
                emits: vec![SchemaId("arcs.observation.request_slice_test.v1".into())],
            },
            required_permissions: vec!["metrics.read".into()],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        boundary: None,
        adapter_id: AdapterId("metrics.request-test".into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("metrics.fetch".into())],
        granted_permissions: vec!["metrics.read".into()],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: Some(SourceKind::Api),
        max_payload_bytes: 1024,
        max_external_reference_bytes: 256,
        data_limits: None,
        input_data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
        data_classification: None,
    }
}

fn input() -> Artifact {
    Artifact::new(
        "request-1",
        "request-1-v1",
        "request",
        "arcs.request.slice_test.v1",
        "2026-08-01T11:59:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs.runtime".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "cycle-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "cycle:1",
        json!({"metric": "cpu_usage"}),
    )
    .with_subject("server-01/cpu")
}

#[test]
fn response_is_correlated_persisted_and_replay_protected_inside_request_slice() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(endpoint_manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::clone(&calls),
        mismatched_correlation: false,
    };
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);
    let capability = CapabilityRef::new("metrics.request-test", "metrics.fetch");
    let response_schema = SchemaId("arcs.observation.request_slice_test.v1".into());

    let response = service
        .execute(&endpoint, &capability, &input.version_id, &response_schema)
        .unwrap();
    let replay = service
        .execute(&endpoint, &capability, &input.version_id, &response_schema)
        .unwrap();

    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(replay, response);
    assert_eq!(response.subject, Some(SubjectId("server-01/cpu".into())));
    assert_eq!(response.trust.level, TrustLevel::Medium);
    assert!(store.len().unwrap() >= 4);
}

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        Self {
            path: std::env::temp_dir().join(format!(
                "arcs-request-invocation-{}-{sequence}.sqlite",
                std::process::id()
            )),
        }
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn successful_request_is_not_dispatched_again_after_restart() {
    let schemas = schemas();
    let database = TemporaryDatabase::new();
    let input = input();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        registry
            .register(endpoint_manifest.clone(), grant(), &schemas, &store)
            .unwrap();
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::clone(&calls),
        mismatched_correlation: false,
    };
    let capability = CapabilityRef::new("metrics.request-test", "metrics.fetch");
    let response_schema = SchemaId("arcs.observation.request_slice_test.v1".into());

    let first = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        store.append(&input, &schemas).unwrap();
        let mut ids = TestIds;
        RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .execute(&endpoint, &capability, &input.version_id, &response_schema)
            .unwrap()
    };
    let replay = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        let mut ids = TestIds;
        RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .execute(&endpoint, &capability, &input.version_id, &response_schema)
            .unwrap()
    };

    assert_eq!(replay, first);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn dispatched_request_is_recovered_with_the_same_invocation_id() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(endpoint_manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::clone(&calls),
        mismatched_correlation: false,
    };
    let capability = CapabilityRef::new("metrics.request-test", "metrics.fetch");
    let response_schema = SchemaId("arcs.observation.request_slice_test.v1".into());
    let invocation_id = invocation_id(&capability, &input.version_id, &response_schema);
    let invocations = InvocationService::new(&store, &schemas, &FixedClock);
    let prepared = invocations
        .prepare(InvocationSpec {
            invocation_id: invocation_id.clone(),
            kind: InvocationKind::Request,
            capability: capability_name(&capability),
            input_version: input.version_id.clone(),
            input_fingerprint: crate::runtime::deterministic_input_fingerprint(&[&invocation_id]),
        })
        .unwrap();
    invocations.dispatch(&prepared).unwrap();

    let mut ids = TestIds;
    let response = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &capability, &input.version_id, &response_schema)
        .unwrap();

    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap()[0].invocation_id, invocation_id);
    assert_eq!(
        InvocationService::new(&store, &schemas, &FixedClock)
            .lookup(&invocation_id)
            .unwrap()
            .unwrap()
            .status,
        InvocationStatus::Succeeded
    );
    assert_eq!(store.get(&response.version_id).unwrap(), Some(response));
}

#[test]
fn mismatched_external_correlation_records_failed_invocation_without_result() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(endpoint_manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::clone(&calls),
        mismatched_correlation: true,
    };
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);

    let result = service.execute(
        &endpoint,
        &CapabilityRef::new("metrics.request-test", "metrics.fetch"),
        &input.version_id,
        &SchemaId("arcs.observation.request_slice_test.v1".into()),
    );

    assert!(matches!(
        result,
        Err(RequestError::InvocationResponseMismatch)
    ));
    let invocation_id = calls.lock().unwrap()[0].invocation_id.clone();
    let invocation = InvocationService::new(&store, &schemas, &FixedClock)
        .lookup(&invocation_id)
        .unwrap()
        .unwrap();

    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(invocation.status, InvocationStatus::Failed);
    assert_eq!(invocation.result_version, None);
    assert_eq!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.observation.request_slice_test.v1".into()),
            )
            .unwrap(),
        None
    );
}

fn split_fixture() -> (
    SchemaRegistry,
    SqliteArtifactStore,
    AdapterRegistry,
    RecordingEndpoint,
) {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store.append(&input(), &schemas).unwrap();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest(), grant(), &schemas, &store)
        .unwrap();
    let endpoint = RecordingEndpoint {
        manifest: manifest(),
        calls: Arc::new(Mutex::new(Vec::new())),
        mismatched_correlation: false,
    };
    (schemas, store, registry, endpoint)
}
fn prepare_split(
    service: &mut RequestService<'_>,
    endpoint: &RecordingEndpoint,
) -> PreparedRequest {
    match service
        .prepare(
            endpoint,
            &CapabilityRef::new("metrics.request-test", "metrics.fetch"),
            &input().version_id,
            &SchemaId("arcs.observation.request_slice_test.v1".into()),
        )
        .unwrap()
    {
        RequestPreparation::Dispatch(prepared) => *prepared,
        RequestPreparation::Complete(_) => panic!("expected new dispatch"),
    }
}
#[test]
fn worker_preparation_reports_a_recorded_failure_and_direct_retry_keeps_its_id() {
    use crate::adapters::AdapterCallError;

    let (schemas, store, registry, endpoint) = split_fixture();
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);
    let prepared = prepare_split(&mut service, &endpoint);
    let invocation_id = prepared.invocation().invocation_id.clone();
    let capability = prepared.invocation().capability.clone();
    let input = prepared.invocation().request_version_id.clone();
    let schema = prepared.invocation().response_schema_id.clone();
    assert!(matches!(
        service.finish(prepared, Err(AdapterCallError::Timeout)),
        Err(RequestError::AdapterCall(AdapterCallError::Timeout))
    ));

    let recorded = service.prepare(&endpoint, &capability, &input, &schema);
    assert!(matches!(
        recorded,
        Err(RequestError::RecordedFailure { invocation_id: saved, .. }) if saved == invocation_id
    ));
    assert!(endpoint.calls.lock().unwrap().is_empty());
    service
        .execute(&endpoint, &capability, &input, &schema)
        .unwrap();
    let calls = endpoint.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].invocation_id, invocation_id);
}

#[test]
fn preparation_does_not_call_the_endpoint_and_finish_retains_correlated_replay() {
    let (schemas, store, registry, endpoint) = split_fixture();
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);
    let prepared = prepare_split(&mut service, &endpoint);
    assert!(endpoint.calls.lock().unwrap().is_empty());
    let response = endpoint.fetch(prepared.invocation());
    let artifact = service.finish(prepared, response).unwrap();
    let replay = service
        .execute(
            &endpoint,
            &CapabilityRef::new("metrics.request-test", "metrics.fetch"),
            &input().version_id,
            &artifact.schema_id,
        )
        .unwrap();
    assert_eq!(artifact, replay);
    assert_eq!(endpoint.calls.lock().unwrap().len(), 1);
}
#[test]
fn a_late_response_cannot_complete_an_obsolete_dispatch() {
    let (schemas, store, registry, endpoint) = split_fixture();
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);
    let first = prepare_split(&mut service, &endpoint);
    let later = prepare_split(&mut service, &endpoint);
    assert_eq!(first.invocation(), later.invocation());
    let response = endpoint.fetch(first.invocation());
    assert!(matches!(
        service.finish(first, response),
        Err(RequestError::StaleDispatch)
    ));
    assert!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.observation.request_slice_test.v1".into())
            )
            .unwrap()
            .is_none()
    );
    let response = endpoint.fetch(later.invocation());
    assert!(service.finish(later, response).is_ok());
}
#[test]
fn changing_the_authorizing_grant_rejects_a_prepared_response() {
    let (schemas, store, registry, endpoint) = split_fixture();
    let mut ids = TestIds;
    let prepared = prepare_split(
        &mut RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock),
        &endpoint,
    );
    let mut changed = grant();
    changed.assigned_trust = TrustLevel::High;
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest(), changed, &schemas, &store)
        .unwrap();
    let response = endpoint.fetch(prepared.invocation());
    let result = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .finish(prepared, response);
    assert!(matches!(result, Err(RequestError::PreparedPolicyChanged)));
    assert!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.observation.request_slice_test.v1".into())
            )
            .unwrap()
            .is_none()
    );
}

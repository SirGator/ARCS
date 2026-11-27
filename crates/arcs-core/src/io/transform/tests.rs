use std::sync::Mutex;

use super::*;
use crate::adapters::{
    AdapterGrant, AdapterId, AdapterRegistry, CapabilityContract, CapabilityDescriptor,
    CapabilityId, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, SchemaId, SchemaRegistry, SequenceIdGenerator, Source, SourceClass,
    SourceKind, SystemClock, Trust, TrustLevel, VersionId,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_invocation_id,
};
use crate::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use serde_json::json;

const INPUT: &str = "arcs.raw.transform_test.v1";
const MID: &str = "arcs.mid.transform_test.v1";
const OUTPUT: &str = "arcs.normalized.transform_test.v1";
const PROJECTION_INPUT: &str = "arcs.raw.transform_projection_test.v1";
const PROJECTION_SCHEMA: &str = "arcs.projection.transform_test.v1";

struct Endpoint {
    manifest: AdapterManifest,
    calls: Mutex<Vec<TransformInvocation>>,
    failure: u8,
}
impl TransformAdapter for Endpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn transform(
        &self,
        invocation: &TransformInvocation,
    ) -> Result<TransformResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(invocation.clone());
        if self.failure == 4 {
            return Err(AdapterCallError::Timeout);
        }
        Ok(TransformResponse {
            invocation_id: if self.failure == 1 {
                "wrong".into()
            } else {
                invocation.invocation_id.clone()
            },
            external_reference: if self.failure == 5 {
                "x".repeat(257)
            } else {
                "transform:test".into()
            },
            payload: match self.failure {
                2 => json!({"value":"wrong"}),
                3 => json!({"value":1,"junk":"x".repeat(5000)}),
                _ => json!({"value":invocation.input_payload["value"].as_f64().unwrap() * 2.0}),
            },
        })
    }
}

struct Fixture {
    schemas: SchemaRegistry,
    store: SqliteArtifactStore,
    endpoint: Endpoint,
    grant: AdapterGrant,
}
impl Fixture {
    fn new() -> Self {
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        for id in [INPUT, OUTPUT] {
            schemas.register_json(&json!({"$id":id,"type":"object","required":["value"],"properties":{"value":{"type":"number"}},"additionalProperties":false}).to_string()).unwrap();
        }
        let manifest = AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("transform.test".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                description: None,
                id: CapabilityId("double".into()),
                contract: CapabilityContract::Transform {
                    accepts: vec![SchemaId(INPUT.into())],
                    emits: vec![SchemaId(OUTPUT.into())],
                },
                required_permissions: vec!["data.transform".into()],
            }],
        };
        let grant = AdapterGrant {
            adapter_id: manifest.adapter_id.clone(),
            producer_class: ProducerClass::Adapter,
            enabled_capabilities: vec![CapabilityId("double".into())],
            granted_permissions: vec!["data.transform".into()],
            assigned_trust: TrustLevel::High,
            ingress_source_kind: None,
            max_payload_bytes: 4096,
            max_external_reference_bytes: 256,
            input_data_limits: None,
            reasoning_limits_v2: None,
            reasoning_limits: None,
            data_limits: None,
            data_classification: None,
        };
        let fixture = Self {
            schemas,
            store: SqliteArtifactStore::in_memory().unwrap(),
            endpoint: Endpoint {
                manifest,
                calls: Mutex::new(vec![]),
                failure: 0,
            },
            grant,
        };
        fixture
            .store
            .append(&input("input-v1", TrustLevel::Medium), &fixture.schemas)
            .unwrap();
        fixture
    }
    fn registry(&self) -> AdapterRegistry {
        let mut registry = AdapterRegistry::new();
        registry
            .register(
                self.endpoint.manifest.clone(),
                self.grant.clone(),
                &self.schemas,
                &self.store,
            )
            .unwrap();
        registry
    }
    fn run(&self, input: &str) -> Result<Artifact, TransformError> {
        TransformService::new(
            &self.registry(),
            &self.schemas,
            &self.store,
            &mut self.store.durable_id_generator("transform-test").unwrap(),
            &SystemClock,
        )
        .execute(
            &self.endpoint,
            &capability(),
            &VersionId(input.into()),
            &SchemaId(OUTPUT.into()),
        )
    }
}
fn capability() -> CapabilityRef {
    CapabilityRef::new("transform.test", "double")
}

fn register_route_capability(
    fixture: &Fixture,
    registry: &mut AdapterRegistry,
    adapter_id: &str,
    capability_id: &str,
    accepts: &[&str],
    emits: &[&str],
) {
    let manifest = AdapterManifest {
        protocol_version: 1,
        adapter_id: AdapterId(adapter_id.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            description: None,
            id: CapabilityId(capability_id.into()),
            contract: CapabilityContract::Transform {
                accepts: accepts.iter().map(|id| SchemaId((*id).into())).collect(),
                emits: emits.iter().map(|id| SchemaId((*id).into())).collect(),
            },
            required_permissions: vec![],
        }],
    };
    let grant = AdapterGrant {
        adapter_id: manifest.adapter_id.clone(),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId(capability_id.into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::High,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        input_data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
        data_limits: None,
        data_classification: None,
    };
    registry
        .register(manifest, grant, &fixture.schemas, &fixture.store)
        .unwrap();
}

fn routing_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture
        .schemas
        .register_json(
            &json!({
                "$id": MID,
                "type":"object",
                "required":["value"],
                "properties":{"value":{"type":"number"}},
                "additionalProperties":false
            })
            .to_string(),
        )
        .unwrap();
    fixture
}

fn input(version: &str, level: TrustLevel) -> Artifact {
    Artifact::new(
        format!("artifact-{version}"),
        version,
        "raw",
        INPUT,
        "2026-10-23T00:00:00Z",
        Actor {
            actor_type: ActorType::Adapter,
            id: "source".into(),
        },
        Source {
            kind: SourceKind::Sensor,
            reference: version.into(),
        },
        Trust {
            level,
            source_class: SourceClass::External,
        },
        format!("source:{version}"),
        json!({"value":12.0}),
    )
    .with_subject("measurement-1")
}

fn projection_fixture() -> (SchemaRegistry, SqliteArtifactStore, VersionId) {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(
            &json!({
                "$id": PROJECTION_INPUT,
                "type": "object",
                "required": ["public", "private"],
                "properties": {
                    "public": {"type": "string"},
                    "private": {"type": "string"}
                },
                "additionalProperties": false
            })
            .to_string(),
        )
        .unwrap();
    schemas
        .register_json(
            &json!({
                "$id": PROJECTION_SCHEMA,
                "type": "object",
                "required": ["public"],
                "properties": {"public": {"type": "string"}},
                "additionalProperties": false
            })
            .to_string(),
        )
        .unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = Artifact::new(
        "projection-input",
        "projection-input-v1",
        "raw",
        PROJECTION_INPUT,
        "2026-10-23T00:00:00Z",
        Actor {
            actor_type: ActorType::Adapter,
            id: "source".into(),
        },
        Source {
            kind: SourceKind::Sensor,
            reference: "projection-input".into(),
        },
        Trust {
            level: TrustLevel::Medium,
            source_class: SourceClass::External,
        },
        "source:projection-input",
        json!({"public":"visible", "private":"hidden"}),
    );
    store.append(&source, &schemas).unwrap();
    (schemas, store, VersionId("projection-input-v1".into()))
}

#[test]
fn input_projection_is_a_subset_with_stable_reuse_and_preserved_origin_boundary() {
    let (schemas, store, source_version) = projection_fixture();
    let projection = TransformInputProjection {
        schema_id: SchemaId(PROJECTION_SCHEMA.into()),
        payload_fields: vec!["public".into()],
    };
    let first = projection
        .materialize(&source_version, &store, &schemas, &SystemClock)
        .unwrap();
    let count = store.len().unwrap();

    assert_eq!(first.payload, json!({"public": "visible"}));
    assert_eq!(first.created_by.actor_type, ActorType::System);
    assert_eq!(first.created_by.id, PROJECTION_PUBLISHER);
    assert_eq!(first.trust.source_class, SourceClass::External);
    assert_eq!(first.trust.level, TrustLevel::Medium);
    assert_eq!(
        first.provenance.as_ref().unwrap().parents,
        vec![source_version.0.clone()]
    );
    assert_eq!(first.source.kind, SourceKind::Internal);
    let relations = ArtifactRelations::new(&store)
        .outgoing(&first.version_id)
        .unwrap();
    assert!(
        relations
            .iter()
            .any(|edge| edge.to == source_version && edge.kind == relation_kinds::derived_from())
    );
    let second = projection
        .materialize(&source_version, &store, &schemas, &SystemClock)
        .unwrap();
    assert_eq!(second, first);
    assert_eq!(store.len().unwrap(), count);
}

#[test]
fn input_projection_rejects_missing_fields_and_non_projection_policies_without_artifacts() {
    let (schemas, store, source_version) = projection_fixture();
    let count = store.len().unwrap();
    let missing = TransformInputProjection {
        schema_id: SchemaId(PROJECTION_SCHEMA.into()),
        payload_fields: vec!["private.nested".into()],
    };
    assert!(matches!(
        missing.materialize(&source_version, &store, &schemas, &SystemClock),
        Err(TransformError::MissingProjectedField(field)) if field == "private.nested"
    ));
    let invalid_payload = TransformInputProjection {
        schema_id: SchemaId(PROJECTION_SCHEMA.into()),
        payload_fields: vec!["private".into()],
    };
    assert!(matches!(
        invalid_payload.materialize(&source_version, &store, &schemas, &SystemClock),
        Err(TransformError::InvalidProjectionPayload(_))
    ));
    let invalid_policy = TransformInputProjection {
        schema_id: SchemaId(PROJECTION_INPUT.into()),
        payload_fields: vec!["public".into()],
    };
    assert!(matches!(
        invalid_policy.materialize(&source_version, &store, &schemas, &SystemClock),
        Err(TransformError::InvalidProjectionPolicy(_))
    ));
    assert_eq!(store.len().unwrap(), count);
}

#[test]
fn result_is_derived_audited_and_replayed_without_new_artifacts_or_current_state() {
    let fixture = Fixture::new();
    let result = fixture.run("input-v1").unwrap();
    let count = fixture.store.len().unwrap();
    assert_eq!(result.payload, json!({"value":24.0}));
    assert_eq!(result.trust.level, TrustLevel::Medium);
    assert_eq!(
        result.provenance.as_ref().unwrap().parents,
        vec!["input-v1"]
    );
    assert_eq!(result.created_by.actor_type, ActorType::Adapter);
    assert!(
        ArtifactRelations::new(&fixture.store)
            .outgoing(&result.version_id)
            .unwrap()
            .iter()
            .any(|relation| relation.to.0 == "input-v1"
                && relation.kind == relation_kinds::derived_from())
    );
    assert!(
        fixture
            .store
            .current(result.subject.as_ref().unwrap(), &result.schema_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.run("input-v1").unwrap(), result);
    assert_eq!(fixture.store.len().unwrap(), count);
    assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
}

#[test]
fn worker_preparation_defers_endpoint_and_finish_persists_for_synchronous_replay() {
    let fixture = Fixture::new();
    let registry = fixture.registry();
    let initial_count = fixture.store.len().unwrap();
    let mut ids = SequenceIdGenerator::new("transform-worker-test");
    let mut service = TransformService::new(
        &registry,
        &fixture.schemas,
        &fixture.store,
        &mut ids,
        &SystemClock,
    );

    let preparation = service
        .prepare_for_worker(
            &fixture.endpoint,
            &capability(),
            &VersionId("input-v1".into()),
            &SchemaId(OUTPUT.into()),
        )
        .unwrap();
    let prepared = match preparation {
        TransformPreparation::Dispatch(prepared) => prepared,
        TransformPreparation::Complete(_) => panic!("first request must dispatch"),
    };
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
    assert!(
        fixture
            .store
            .committed_after(initial_count)
            .unwrap()
            .iter()
            .all(|entry| entry.artifact.schema_id.0 != OUTPUT)
    );

    let response = fixture.endpoint.transform(prepared.invocation()).unwrap();
    let result = service.finish(*prepared, Ok(response)).unwrap();
    assert_eq!(result.payload, json!({"value": 24.0}));
    assert!(
        fixture
            .store
            .committed_after(initial_count)
            .unwrap()
            .iter()
            .any(|entry| entry.artifact.schema_id.0 == OUTPUT)
    );
    drop(service);

    assert_eq!(fixture.run("input-v1").unwrap(), result);
    assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
}

#[test]
fn missing_input_wrong_endpoint_and_input_limit_fail_before_dispatch() {
    let mut fixture = Fixture::new();
    let count = fixture.store.len().unwrap();
    assert!(matches!(
        fixture.run("missing"),
        Err(TransformError::MissingInput(_))
    ));
    let registry = fixture.registry();
    fixture.endpoint.manifest.adapter_version = "changed".into();
    let mut ids = SequenceIdGenerator::new("test");
    assert!(matches!(
        TransformService::new(
            &registry,
            &fixture.schemas,
            &fixture.store,
            &mut ids,
            &SystemClock
        )
        .execute(
            &fixture.endpoint,
            &capability(),
            &VersionId("input-v1".into()),
            &SchemaId(OUTPUT.into())
        ),
        Err(TransformError::EndpointMismatch)
    ));
    fixture.endpoint.manifest.adapter_version = "1.0.0".into();
    fixture.grant.max_payload_bytes = 1;
    assert!(matches!(
        fixture.run("input-v1"),
        Err(TransformError::InputTooLarge)
    ));
    assert_eq!(fixture.store.len().unwrap(), count);
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
}

#[test]
fn invalid_outputs_are_terminal_and_do_not_commit_a_result() {
    for failure in [1, 2, 3, 5] {
        let mut fixture = Fixture::new();
        fixture.endpoint.failure = failure;
        let error = fixture.run("input-v1").unwrap_err();
        assert!(matches!(
            error,
            TransformError::InvocationResponseMismatch
                | TransformError::InvalidOutput(_)
                | TransformError::OutputTooLarge
                | TransformError::InvalidExternalReference
        ));
        assert!(matches!(
            fixture.run("input-v1"),
            Err(TransformError::Invocation(_))
        ));
        assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
        assert!(
            fixture
                .store
                .committed_after(0)
                .unwrap()
                .iter()
                .all(|entry| entry.artifact.schema_id.0 != OUTPUT)
        );
    }
}

#[test]
fn interrupted_dispatch_is_recovered_with_the_same_id_and_timeout_can_be_retried() {
    let mut fixture = Fixture::new();
    fixture.endpoint.failure = 4;
    assert!(matches!(
        fixture.run("input-v1"),
        Err(TransformError::AdapterCall(AdapterCallError::Timeout))
    ));
    let invocation = fixture.endpoint.calls.lock().unwrap()[0].clone();
    let service = InvocationService::new(&fixture.store, &fixture.schemas, &SystemClock);
    let failed = service.lookup(&invocation.invocation_id).unwrap().unwrap();
    assert!(failed.retryable);
    let recovered = service.recover(&failed).unwrap();
    let dispatched = service.dispatch(&recovered).unwrap();
    assert_eq!(dispatched.status, InvocationStatus::Dispatched);
    fixture.endpoint.failure = 0;
    fixture.run("input-v1").unwrap();
    assert_eq!(
        fixture.endpoint.calls.lock().unwrap()[1].invocation_id,
        invocation.invocation_id
    );
    let artifact = fixture
        .store
        .committed_after(0)
        .unwrap()
        .into_iter()
        .find(|entry| entry.artifact.schema_id.0 == "arcs.invocation.v2")
        .unwrap()
        .artifact;
    assert_eq!(artifact.payload["kind"], "transform");
}

#[test]
fn transform_cannot_create_control_artifacts_even_if_manifest_offers_them() {
    for schema in [
        "arcs.action.v1",
        "arcs.approval.v1",
        "arcs.verification_report.v1",
        "arcs.execution_result.v1",
        "arcs.invocation.v2",
        "arcs.outcome.v1",
        "arcs.output_request.v1",
    ] {
        let mut fixture = Fixture::new();
        fixture.endpoint.manifest.capabilities[0].contract = CapabilityContract::Transform {
            accepts: vec![SchemaId(INPUT.into())],
            emits: vec![SchemaId(schema.into())],
        };
        let registry = fixture.registry();
        let count = fixture.store.len().unwrap();
        assert!(matches!(
            TransformService::new(
                &registry,
                &fixture.schemas,
                &fixture.store,
                &mut SequenceIdGenerator::new("test"),
                &SystemClock
            )
            .execute(
                &fixture.endpoint,
                &capability(),
                &VersionId("input-v1".into()),
                &SchemaId(schema.into())
            ),
            Err(TransformError::ControlOutputForbidden(_))
        ));
        assert_eq!(fixture.store.len().unwrap(), count);
        assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn model_origin_survives_system_transform_and_model_producers_cannot_transform() {
    let mut fixture = Fixture::new();
    let mut model_input = input("model-input", TrustLevel::High);
    model_input.created_by.actor_type = ActorType::Model;
    model_input.trust.source_class = SourceClass::Model;
    fixture
        .store
        .append(&model_input, &fixture.schemas)
        .unwrap();
    fixture.grant.producer_class = ProducerClass::System;
    let result = fixture.run("model-input").unwrap();
    assert_eq!(result.trust.level, TrustLevel::Low);
    assert_eq!(result.trust.source_class, SourceClass::Model);
    fixture.grant.producer_class = ProducerClass::Model;
    assert!(matches!(
        fixture.run("input-v1"),
        Err(TransformError::ModelMustUseReasoning)
    ));
}

#[test]
fn replay_checks_current_grants_and_new_adapter_versions_produce_new_results() {
    let mut fixture = Fixture::new();
    let first = fixture.run("input-v1").unwrap();
    fixture.grant.assigned_trust = TrustLevel::Low;
    assert!(matches!(
        fixture.run("input-v1"),
        Err(TransformError::CachedResultNotPermitted)
    ));
    fixture.grant.assigned_trust = TrustLevel::High;
    fixture.endpoint.manifest.adapter_version = "2.0.0".into();
    let second = fixture.run("input-v1").unwrap();
    assert_ne!(first.version_id, second.version_id);
    assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 2);
}

#[test]
fn invocation_identity_conflict_does_not_return_another_cached_result() {
    let fixture = Fixture::new();
    let id = deterministic_invocation_id(
        InvocationKind::Transform,
        &["transform.test", "double", "input-v1", OUTPUT, "1.0.0"],
    );
    InvocationService::new(&fixture.store, &fixture.schemas, &SystemClock)
        .prepare(InvocationSpec {
            invocation_id: id,
            kind: InvocationKind::Transform,
            capability: "transform.test/double".into(),
            input_version: VersionId("input-v1".into()),
            input_fingerprint: "different identity".into(),
        })
        .unwrap();
    assert!(matches!(
        fixture.run("input-v1"),
        Err(TransformError::Invocation(_))
    ));
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
}

#[test]
fn routing_is_deterministic_for_equal_successors_even_when_allowlist_is_reversed() {
    let fixture = routing_fixture();
    let mut registry = AdapterRegistry::new();
    register_route_capability(&fixture, &mut registry, "z.route", "step", &[INPUT], &[MID]);
    register_route_capability(&fixture, &mut registry, "a.route", "step", &[INPUT], &[MID]);
    register_route_capability(
        &fixture,
        &mut registry,
        "finish.route",
        "step",
        &[MID],
        &[OUTPUT],
    );
    let first = TransformRouting {
        output_schema_id: SchemaId(OUTPUT.into()),
        allowed_capabilities: vec![
            CapabilityRef::new("z.route", "step"),
            CapabilityRef::new("finish.route", "step"),
            CapabilityRef::new("a.route", "step"),
        ],
        max_steps: 2,
    }
    .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into()))
    .unwrap()
    .unwrap();
    let second = TransformRouting {
        output_schema_id: SchemaId(OUTPUT.into()),
        allowed_capabilities: vec![
            CapabilityRef::new("a.route", "step"),
            CapabilityRef::new("finish.route", "step"),
            CapabilityRef::new("z.route", "step"),
        ],
        max_steps: 2,
    }
    .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into()))
    .unwrap()
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first[0].capability, CapabilityRef::new("a.route", "step"));
}

#[test]
fn routing_prefers_shortest_path_and_enforces_zero_hop_limits_and_grants() {
    let fixture = routing_fixture();
    let mut registry = AdapterRegistry::new();
    register_route_capability(
        &fixture,
        &mut registry,
        "long.route",
        "step",
        &[INPUT],
        &[MID],
    );
    register_route_capability(
        &fixture,
        &mut registry,
        "finish.route",
        "step",
        &[MID],
        &[OUTPUT],
    );
    register_route_capability(
        &fixture,
        &mut registry,
        "direct.route",
        "step",
        &[INPUT],
        &[OUTPUT],
    );
    let shortest = TransformRouting {
        output_schema_id: SchemaId(OUTPUT.into()),
        allowed_capabilities: vec![
            CapabilityRef::new("long.route", "step"),
            CapabilityRef::new("finish.route", "step"),
            CapabilityRef::new("direct.route", "step"),
        ],
        max_steps: 2,
    }
    .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into()))
    .unwrap()
    .unwrap();
    assert_eq!(shortest.len(), 1);
    assert_eq!(
        shortest[0].capability,
        CapabilityRef::new("direct.route", "step")
    );

    let zero_hop = TransformRouting {
        output_schema_id: SchemaId(INPUT.into()),
        allowed_capabilities: vec![],
        max_steps: 1,
    }
    .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into()))
    .unwrap();
    assert_eq!(zero_hop, Some(vec![]));
    assert!(matches!(
        TransformRouting {
            output_schema_id: SchemaId(OUTPUT.into()),
            allowed_capabilities: vec![],
            max_steps: 0,
        }
        .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into())),
        Err(TransformError::InvalidRoutingPolicy(_))
    ));
    assert!(matches!(
        TransformRouting {
            output_schema_id: SchemaId(OUTPUT.into()),
            allowed_capabilities: vec![CapabilityRef::new("transform.test", "missing")],
            max_steps: 1,
        }
        .resolve(&registry, &fixture.schemas, &SchemaId(INPUT.into())),
        Err(TransformError::Authorization(_))
    ));
}

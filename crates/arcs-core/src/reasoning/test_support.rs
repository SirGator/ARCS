use super::general::*;
use crate::adapters::{AdapterCallError, AdapterGrant, AdapterManifest, AdapterRegistry};
use crate::core::*;
use crate::reasoning::*;
use crate::store::SqliteArtifactStore;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) struct Peer {
    pub(crate) manifest: AdapterManifest,
    pub(crate) calls: AtomicUsize,
}
impl GeneralReasoningAdapter for Peer {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn propose_v2(
        &self,
        invocation: &ReasoningInvocationV2,
    ) -> Result<ReasoningResponseV2, AdapterCallError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(serde_json::from_value(json!({
            "invocation_id":invocation.invocation_id,"request_id":invocation.request_id,"status":"solved",
            "candidates":[{"schema_id":invocation.target_schema_id,"required_capabilities":[],"referenced_versions":["input-v1"],"payload":{"summary":"remembered context"}}],
            "trace":{"method":"learned","implementation":"test-peer","version":"1","iterations":null,"output_tokens":null,"details":{}}
        })).unwrap())
    }
}
pub(crate) struct Fixture {
    pub(crate) store: SqliteArtifactStore,
    pub(crate) schemas: SchemaRegistry,
    pub(crate) registry: AdapterRegistry,
    pub(crate) ids: SequenceIdGenerator,
    pub(crate) peer: Peer,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let store = SqliteArtifactStore::in_memory().unwrap();
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        schemas.register_json(r#"{"$id":"arcs.memory_candidate.test.v1","type":"object","required":["summary"],"properties":{"summary":{"type":"string"}},"additionalProperties":false}"#).unwrap();
        let manifest=serde_json::from_value(json!({"protocol_version":1,"adapter_id":"peer","adapter_version":"1","capabilities":[{"id":"propose","contract":{"kind":"reason","emits":["arcs.memory_candidate.test.v1"]},"required_permissions":[]}]})).unwrap();
        let peer = Peer {
            manifest,
            calls: AtomicUsize::new(0),
        };
        let mut registry = AdapterRegistry::new();
        registry
            .register(peer.manifest.clone(), Self::grant(), &schemas, &store)
            .unwrap();
        let input = Artifact::new(
            "input",
            "input-v1",
            "input",
            "arcs.input.v1",
            "2026-11-15T00:00:00Z",
            Actor {
                actor_type: ActorType::Human,
                id: "user".into(),
            },
            Source {
                kind: SourceKind::Chat,
                reference: "test".into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::Human,
            },
            "test",
            json!({"raw_text":"continue"}),
        );
        store.append(&input, &schemas).unwrap();
        let ids = store.durable_id_generator("split-test").unwrap();
        Self {
            store,
            schemas,
            registry,
            ids,
            peer,
        }
    }
    pub(crate) fn grant() -> AdapterGrant {
        serde_json::from_value(json!({"adapter_id":"peer","producer_class":"model","enabled_capabilities":["propose"],"granted_permissions":[],"assigned_trust":"medium","ingress_source_kind":null,"max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null,"reasoning_limits_v2":{"methods":["learned"],"resources":{"max_context_items":4,"max_context_bytes":16384,"max_output_bytes":4096,"max_candidates":4,"max_runtime_ms":1000,"max_iterations":null,"max_output_tokens":null}}})).unwrap()
    }
    pub(crate) fn service(&mut self) -> GeneralReasoningService<'_> {
        GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
    }
    pub(crate) fn request() -> ReasoningRequestV2 {
        ReasoningRequestV2 {
            request_id: "request".into(),
            capability: crate::adapters::CapabilityRef::new("peer", "propose"),
            method: ReasoningMethod::Learned,
            objective: "continue".into(),
            context: vec![ContextSelection {
                version_id: VersionId("input-v1".into()),
                payload_fields: vec!["raw_text".into()],
            }],
            target_schema_id: SchemaId("arcs.memory_candidate.test.v1".into()),
            allowed_capabilities: vec![],
            constraints: json!({}),
            budget: ReasoningResources {
                max_context_items: 1,
                max_context_bytes: 8192,
                max_output_bytes: 4096,
                max_candidates: 1,
                max_runtime_ms: 1000,
                max_iterations: None,
                max_output_tokens: None,
            },
        }
    }
    pub(crate) fn prepare(&mut self) -> ReasoningPreparation {
        GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
        .prepare(&self.peer, Self::request(), &[])
        .unwrap()
    }
    pub(crate) fn dispatch(&mut self) -> PreparedReasoning {
        match self.prepare() {
            ReasoningPreparation::Dispatch(prepared) => *prepared,
            _ => panic!("expected dispatch"),
        }
    }
}

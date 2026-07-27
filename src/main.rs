use arcs::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGateway, AdapterGrant, AdapterId, AdapterManifest,
    BoundarySubmission, CapabilityContract, CapabilityDescriptor, CapabilityId, ProducerClass,
    SequenceIdGenerator, SystemClock,
};
use arcs::core::{SchemaId, SchemaRegistry, SourceKind, SubjectId, TrustLevel};
use arcs::store::SqliteArtifactStore;
use serde_json::json;

fn main() {
    // Die Demo verwendet dieselben eingebetteten Verträge wie der Core.
    let mut schemas =
        SchemaRegistry::with_bundled_schemas().expect("bundled schemas must be valid");

    // Der flüchtige Store hält das Beispiel nebenwirkungsfrei. Für dauerhafte
    // Daten steht `SqliteArtifactStore::open` bereit.
    let store = SqliteArtifactStore::in_memory().expect("store must initialize");

    // Die Demo simuliert einen von außen installierten Chat-Adapter. Sein
    // Manifest behauptet eine Fähigkeit; erst der getrennte Betreiber-Grant
    // schaltet sie tatsächlich frei.
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("demo.chat".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("chat.observe".into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId("arcs.input.v1".into())],
            },
            required_permissions: vec![],
        }],
    };
    let grant = AdapterGrant {
        adapter_id: AdapterId("demo.chat".into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("chat.observe".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: Some(SourceKind::Chat),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    };

    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(SystemClock),
        Box::new(SequenceIdGenerator::new("demo")),
    );
    let adapter_session = gateway
        .register_adapter(manifest, grant, &[])
        .expect("demo adapter registration must be valid");

    // Der Adapter liefert ausschließlich Boundary-Daten. IDs, Zeit,
    // Artifact-Typ, Actor, Trust und Provenance setzt der Core.
    let input = gateway
        .submit_boundary(
            &adapter_session,
            BoundarySubmission {
                capability_id: CapabilityId("chat.observe".into()),
                schema_id: SchemaId("arcs.input.v1".into()),
                subject: SubjectId("current_user_request".into()),
                external_reference: "demo-conversation".into(),
                payload: json!({
                    "raw_text": "Hallo ARCS"
                }),
            },
        )
        .expect("valid adapter input must be committed");

    let loaded = store
        .get(&input.version_id)
        .expect("database read must succeed")
        .expect("stored artifact must exist");

    println!(
        "{}",
        serde_json::to_string_pretty(&loaded).expect("artifact must serialize")
    );
}

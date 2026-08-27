use arcs::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, ProducerClass,
};
use arcs::core::{
    SchemaId, SchemaRegistry, SequenceIdGenerator, SourceKind, SystemClock, TrustLevel,
};
use arcs::observation::{ObservationMessage, ObservationService};
use arcs::store::SqliteArtifactStore;
use arcs::world::{WorldReducer, WorldState};
use serde_json::json;

fn main() {
    // Die Demo verwendet dieselben eingebetteten Verträge wie der Core.
    let schemas = SchemaRegistry::with_bundled_schemas().expect("bundled schemas must be valid");

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
        ingress_source_kind: Some(SourceKind::Chat),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    };

    let adapter_id = manifest.adapter_id.clone();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest, grant, &schemas, &store)
        .expect("demo adapter registration must be valid");

    let clock = SystemClock;
    let mut ids = SequenceIdGenerator::new("demo");
    let mut observation = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock);

    // Der Adapter liefert ausschließlich Boundary-Daten. IDs, Zeit,
    // Artifact-Typ, Actor, Trust und Provenance setzt der Core.
    let recorded = observation
        .ingest_recorded(
            &adapter_id,
            ObservationMessage {
                capability_id: CapabilityId("chat.observe".into()),
                external_subject: Some("current_user_request".into()),
                external_reference: "demo-conversation".into(),
                payload: json!({
                    "raw_text": "Hallo ARCS"
                }),
            },
        )
        .expect("valid adapter input must be committed");
    let mut world = WorldState::new();
    WorldReducer::new()
        .reduce(&mut world, &recorded)
        .expect("committed observation must update the world state");
    let input = recorded.into_artifact();

    let loaded = store
        .get(&input.version_id)
        .expect("database read must succeed")
        .expect("stored artifact must exist");

    println!(
        "{}",
        serde_json::to_string_pretty(&loaded).expect("artifact must serialize")
    );
}

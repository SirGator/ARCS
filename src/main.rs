use arcs::core::{
    Actor, ActorType, Artifact, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel,
};
use arcs::store::SqliteArtifactStore;
use serde_json::json;

fn main() {
    // Die Demo verwendet dieselben eingebetteten Verträge wie der Core.
    let registry = SchemaRegistry::with_bundled_schemas().expect("bundled schemas must be valid");

    // Der flüchtige Store hält das Beispiel nebenwirkungsfrei. Für dauerhafte
    // Daten steht `SqliteArtifactStore::open` bereit.
    let store = SqliteArtifactStore::in_memory().expect("store must initialize");

    // Die menschliche Eingabe wird als nachvollziehbares Input-Artefakt erfasst.
    let input = Artifact::new(
        "input-demo",
        "input-demo-v1",
        "input",
        "arcs.input.v1",
        "2026-07-26T23:00:00+02:00",
        Actor {
            actor_type: ActorType::Human,
            id: "demo-user".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "dummy-input".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "demo:input",
        json!({
            "raw_text": "Hallo ARCS"
        }),
    );

    store
        .append(&input, &registry)
        .expect("valid input must be committed");

    let loaded = store
        .get(&input.version_id)
        .expect("database read must succeed")
        .expect("stored artifact must exist");

    println!(
        "{}",
        serde_json::to_string_pretty(&loaded).expect("artifact must serialize")
    );
}

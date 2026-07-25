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

    // Das menschliche Ziel wird als nachvollziehbares Task-Artefakt erfasst.
    let goal = Artifact::new(
        "goal-demo",
        "goal-demo-v1",
        "task",
        "arcs.task.v1",
        "2026-07-25T18:00:00+02:00",
        Actor {
            actor_type: ActorType::Human,
            id: "demo-user".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "demo".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "demo:goal",
        json!({"title": "Demonstrate the ARCS artifact foundation"}),
    );

    store
        .append(&goal, &registry)
        .expect("valid artifact must be committed");

    let loaded = store
        .get(&goal.version_id)
        .expect("database read must succeed")
        .expect("stored artifact must exist");

    println!(
        "{}",
        serde_json::to_string_pretty(&loaded)
            .expect("artifact must serialize")
    );
}

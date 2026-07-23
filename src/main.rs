mod core;

use core::{
    create_raw_artifact_base,
    validate_artifact,
    ArtifactKind,
    SchemaDefinition,
    SchemaId,
    SchemaRegistry,
};

fn main() {
    let mut registry = SchemaRegistry::new();

    registry.register(SchemaDefinition {
        id: SchemaId::new(1),
        name: "raw_text_input".to_string(),
        artifact_kind: ArtifactKind::Input,
    });

    let mut artifact = create_raw_artifact_base(
        1,
        1,
        ArtifactKind::Input,
        "dummy_adapter".to_string(),
        "dummy_input".to_string(),
        "Hallo ARCS".to_string(),
        vec![],
        "2026-07-18T21:00:00+02:00".to_string(),
    );

    let validation_result = validate_artifact(&mut artifact, &registry);

    if let Err(error) = validation_result {
        eprintln!("Validation failed: {error:?}");
    }

    let json = serde_json::to_string_pretty(&artifact)
        .expect("Artifact konnte nicht als JSON serialisiert werden");

    println!("{json}");
}
//! Ein vollständiger Host-Zyklus mit persistentem Store und externem Adapter.

use std::process::Command;
use std::time::Duration;

use arcs::adapters::stdio::StdioAdapter;
use arcs::adapters::{AdapterGrant, AdapterRegistry};
use arcs::core::{SchemaRegistry, SystemClock};
use arcs::store::SqliteArtifactStore;
use arcs::world::observation::{ObservationLog, ObservationService};
use arcs::world::{WorldReducer, WorldState};
use serde_json::json;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err(
            "usage: arcs-adapter-demo <adapter-executable> <database> [profile.json]".into(),
        );
    }
    let store = SqliteArtifactStore::open(&args[1]).map_err(|e| format!("{e:?}"))?;
    let mut ids = store
        .durable_id_generator("stdio-demo")
        .map_err(|e| format!("{e:?}"))?;
    let mut schemas = SchemaRegistry::with_bundled_schemas().map_err(|e| format!("{e:?}"))?;
    schemas
        .register_json(arcs_simulated_sensor::TEMPERATURE_SCHEMA)
        .map_err(|e| format!("{e:?}"))?;
    let grant: AdapterGrant = serde_json::from_str(include_str!("../../config/sensor.grant.json"))
        .map_err(|e| e.to_string())?;
    let mut registry = AdapterRegistry::new();
    let reducer = WorldReducer::new();
    let mut world = WorldState::new();
    for observation in ObservationLog::new(&store)
        .after(None)
        .map_err(|e| format!("{e:?}"))?
    {
        reducer
            .reduce(&mut world, &observation)
            .map_err(|e| format!("{e:?}"))?;
    }
    let mut command = Command::new(&args[0]);
    if let Some(profile) = args.get(2) {
        command.arg(profile);
    }
    let mut adapter = StdioAdapter::connect(
        &mut command,
        Duration::from_secs(5),
        grant,
        &mut registry,
        &schemas,
        &store,
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut observations =
        ObservationService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    while let Some(recorded) = adapter
        .poll(&mut observations)
        .map_err(|e| format!("{e:?}"))?
    {
        // Alte Receipt-Replays sind bereits im aus dem Log aufgebauten State.
        if world
            .cursor()
            .is_none_or(|cursor| recorded.cursor() > cursor)
        {
            reducer
                .reduce(&mut world, &recorded)
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    adapter.shutdown().map_err(|e| format!("{e:?}"))?;
    let estimates: Vec<_> = world
        .estimates()
        .map(|(_, estimate)| {
            json!({
                "entity_id": estimate.entity_id().as_str(),
                "schema_id": estimate.schema_id(),
                "value": estimate.value(),
                "evidence_version": estimate.evidence_version(),
            })
        })
        .collect();
    println!(
        "{}",
        json!({
            "stored_artifacts": store.len().map_err(|e| format!("{e:?}"))?,
            "world_revision": world.revision().get(),
            "estimates": estimates,
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("adapter host: {error}");
        std::process::exit(1);
    }
}

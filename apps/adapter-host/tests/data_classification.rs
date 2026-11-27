//! Schutzstufen wirken über die gesamte Kette: Adaptereingang, Memory und
//! Reasoning-Kontext. Vertrauliche Erinnerungen erreichen nur einen Adapter,
//! dessen Grant die Datenklasse ausdrücklich erlaubt.
#[path = "support/memory.rs"]
mod memory;
#[path = "support/parallel.rs"]
mod parallel;
mod support;

use arcs_host::{Host, task_runtime::TaskRuntime};
use parallel::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Ein Lauf mit einer Route, die Erinnerungen in den Kontext aufnimmt.
///
/// `reasoner_classification` legt die Datenklasse des Reasoners fest. Ohne
/// Angabe darf er nur interne Daten sehen.
fn fixture(reasoner_classification: Option<&str>) -> Fixture {
    let f = Fixture::new();
    f.peer("main", "normal", false);
    f.route("memory", "main");
    let mut config = f.read("runtime.json");
    let route = &mut config["routes"][0];
    route["memory"] = memory::policy("project:p");
    route["budget"]["max_context_items"] = json!(3);
    f.write("runtime.json", config);
    let mut grant = f.read("main-grant.json");
    if let Some(classification) = reasoner_classification {
        grant["data_classification"] = json!(classification);
    }
    f.write("main-grant.json", grant);
    f
}

/// Wartet bis zur ersten eingefrorenen Kontextauswahl.
fn selection(host: &mut Host, runtime: &mut TaskRuntime) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if let Some(record) = memory::snapshots(host).into_iter().next() {
            return record.payload;
        }
        assert!(Instant::now() < end, "no context selection was recorded");
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// Die Versionskennungen der ausgewählten Erinnerungen.
fn selected_versions(payload: &Value) -> Vec<String> {
    payload["selected"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item["selection"]["version_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn confidential_memory_never_reaches_a_reasoner_without_permission() {
    let f = fixture(None);
    let (mut host, mut runtime) = f.open();
    memory::append_with_classification(&host, "internal-note", "project:p", None);
    let confidential = memory::append_with_classification(
        &host,
        "confidential-note",
        "project:p",
        Some("confidential"),
    );
    host.poll_inputs().unwrap();

    let payload = selection(&mut host, &mut runtime);
    let selected = selected_versions(&payload);

    assert!(
        !selected.contains(&confidential.version_id.0),
        "confidential memory reached the reasoner: {selected:?}"
    );
}

#[test]
fn confidential_memory_reaches_a_reasoner_with_matching_permission() {
    let f = fixture(Some("confidential"));
    let (mut host, mut runtime) = f.open();
    let confidential = memory::append_with_classification(
        &host,
        "confidential-note",
        "project:p",
        Some("confidential"),
    );
    host.poll_inputs().unwrap();

    let payload = selection(&mut host, &mut runtime);
    let selected = selected_versions(&payload);

    assert!(
        selected.contains(&confidential.version_id.0),
        "confidential memory was withheld from an allowed reasoner: {selected:?}"
    );
}

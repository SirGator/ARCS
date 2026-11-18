#[path = "support/parallel.rs"]
mod parallel_support;
mod support;

use arcs::core::*;
use arcs::memory::{MemoryAssessmentCommand, MemoryStatus};
use arcs_host::{Host, task_runtime::TaskRuntime};
use parallel_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn setup() -> Fixture {
    let f = Fixture::new();
    f.peer("a", "normal", false);
    f.route("main", "a");
    f.write("note-schema.json", json!({"$id":"arcs.memory_note.test.v1","type":"object","required":["body","private_note"],"properties":{"body":{"type":"string"},"private_note":{"type":"string"}},"additionalProperties":false}));
    f.write("notes-profile.json", json!({"adapter_id":"memory.input","capability_id":"memory.capture","schema_id":"arcs.memory_note.test.v1","events":[]}));
    f.write("notes-grant.json", json!({"adapter_id":"memory.input","producer_class":"adapter","enabled_capabilities":["memory.capture"],"granted_permissions":["input.read"],"assigned_trust":"low","ingress_source_kind":"file","max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null}));
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({"program":support::adapter_binary("arcs-json-input"),"profile":"notes-profile.json","args":[{"path":"notes.sqlite"}],"grant_file":"notes-grant.json"}));
    host["schemas"]
        .as_array_mut()
        .unwrap()
        .push(json!(f.directory.join("note-schema.json")));
    f.write("host.json", host);
    let mut runtime = f.read("runtime.json");
    let route = &mut runtime["routes"][0];
    route["budget"]["max_context_items"] = json!(4);
    route["memory"] = json!({"scopes":[{"filter":{"schema_id":"arcs.memory_note.test.v1","subject":"input:12:memory.input:14:memory.capture:14:project:public"},"payload_fields":["body"]}],"relations":[],"search":{"max_depth":0,"max_visited":32,"max_edges":32,"max_candidates":16},"max_items":3,"max_bytes":4096,"epistemic":{"allow_unassessed":false,"allow_disputed":false}});
    f.write("runtime.json", runtime);
    f
}
fn note(host: &Host, id: &str, project: &str) -> Artifact {
    let mut ids = host
        .store()
        .durable_id_generator("memory-test-ingress")
        .unwrap();
    arcs::input::InputService::new(host.registry(), host.schemas(), host.store(), &mut ids, &SystemClock)
        .ingest(&arcs::adapters::AdapterId("memory.input".into()), arcs::input::InputMessage {
            capability_id: arcs::adapters::CapabilityId("memory.capture".into()),
            external_subject: Some(project.into()), external_reference: id.into(),
            payload: json!({"body":format!("public memory {id}"),"private_note":"PRIVATE_MEMORY_BODY"}),
        }).unwrap()
}
fn command(note: &Artifact, status: MemoryStatus) -> MemoryAssessmentCommand {
    MemoryAssessmentCommand {
        target_version_id: note.version_id.clone(),
        expected_version_id: None,
        decision_by: Actor {
            actor_type: ActorType::Human,
            id: "test-operator".into(),
        },
        reason: "PRIVATE_REVIEW_REASON".into(),
        status,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
        replacement_version_id: None,
    }
}
fn finish(host: &mut Host, runtime: &mut TaskRuntime) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        runtime.tick(host).unwrap();
        if runtime.report(0).unwrap().ready_for_review == 1 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn invocation(f: &Fixture) -> Value {
    let db = rusqlite::Connection::open(f.directory.join("a.sqlite")).unwrap();
    let raw: String = db
        .query_row("SELECT input FROM results LIMIT 1", [], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}
fn cli(f: &Fixture, command: Value) -> std::process::Output {
    f.write("memory-command.json", command);
    std::process::Command::new(support::adapter_binary("arcs-host"))
        .arg("--config")
        .arg(f.directory.join("host.json"))
        .arg("--memory-command")
        .arg(f.directory.join("memory-command.json"))
        .output()
        .unwrap()
}

#[test]
fn actual_reasoner_receives_only_admitted_fields_and_restart_preserves_snapshot() {
    let f = setup();
    let (mut host, mut runtime) = f.open();
    let active = note(&host, "active", "project:public");
    let retracted = note(&host, "retracted", "project:public");
    let foreign = note(&host, "foreign", "project:private");
    let unknown = note(&host, "unknown", "project:public");
    let first = host
        .assess_memory(&command(&active, MemoryStatus::Active))
        .unwrap();
    host.assess_memory(&command(&retracted, MemoryStatus::Retracted))
        .unwrap();
    host.assess_memory(&command(&foreign, MemoryStatus::Active))
        .unwrap();
    finish(&mut host, &mut runtime);
    let wire = invocation(&f);
    let text = wire.to_string();
    let context = wire["context"].as_array().unwrap();
    assert_eq!(context.len(), 2);
    let item = context
        .iter()
        .find(|i| i["version_id"] == active.version_id.0)
        .unwrap();
    assert_eq!(item["trust_level"], "low");
    assert_eq!(item["payload"], json!({"body":"public memory active"}));
    for secret in [
        "PRIVATE_MEMORY_BODY",
        "PRIVATE_REVIEW_REASON",
        &foreign.version_id.0,
        &retracted.version_id.0,
        &unknown.version_id.0,
    ] {
        assert!(!text.contains(secret));
    }
    let mut retract = first.command;
    retract.expected_version_id = Some(first.version_id);
    retract.status = MemoryStatus::Retracted;
    host.assess_memory(&retract).unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    runtime.tick(&mut host).unwrap();
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(invocation(&f), wire);
    assert_eq!(
        host.store().get(&active.version_id).unwrap().unwrap(),
        active
    );
}

#[test]
fn cli_assesses_inspects_and_rejects_stale_revision_without_runtime_work() {
    let f = setup();
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let target = note(&host, "target", "project:public");
    host.shutdown().unwrap();
    drop(host);
    let c = command(&target, MemoryStatus::Active);
    let result = cli(&f, json!({"command":"assess","assessment":c}));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(
        cli(&f, json!({"command":"assess","assessment":c}))
            .status
            .success()
    );
    let inspected = cli(
        &f,
        json!({"command":"inspect","target_version_ids":[target.version_id,"missing-target"]}),
    );
    assert!(inspected.status.success());
    let inspected: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected[0]["assessment"], receipt);
    assert!(inspected[1]["assessment"].is_null());
    let mut stale = c.clone();
    stale.status = MemoryStatus::Retracted;
    assert!(
        !cli(&f, json!({"command":"assess","assessment":stale}))
            .status
            .success()
    );
    stale.expected_version_id =
        Some(serde_json::from_value(receipt["version_id"].clone()).unwrap());
    assert!(
        cli(&f, json!({"command":"assess","assessment":stale}))
            .status
            .success()
    );
    assert_eq!(f.count("a", "attempts"), 0);
    assert!(!f.directory.join("runtime.sqlite").exists());
}

#[test]
fn invalid_cli_contracts_fail_before_opening_core_or_starting_children() {
    let f = setup();
    for value in [
        json!({"command":"inspect","target_version_ids":[]}),
        json!({"command":"inspect","target_version_ids":["a","a"]}),
        json!({"command":"inspect","target_version_ids":["a"],"unknown":true}),
    ] {
        assert!(!cli(&f, value).status.success());
        assert!(!f.directory.join("core.sqlite").exists());
    }
    let mut c = command(
        &Artifact::new(
            "a",
            "a-v1",
            "input",
            "arcs.input.v1",
            "2026-11-18T00:00:00Z",
            Actor {
                actor_type: ActorType::Human,
                id: "x".into(),
            },
            Source {
                kind: SourceKind::Chat,
                reference: "x".into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::Human,
            },
            "x",
            json!({"raw_text":"x"}),
        ),
        MemoryStatus::Active,
    );
    c.decision_by.actor_type = ActorType::Model;
    assert!(
        !cli(&f, json!({"command":"assess","assessment":c}))
            .status
            .success()
    );
    assert!(!f.directory.join("core.sqlite").exists());
    f.write(
        "memory-command.json",
        json!({"command":"inspect","target_version_ids":["a"]}),
    );
    let mixed = std::process::Command::new(support::adapter_binary("arcs-host"))
        .arg("--config")
        .arg(f.directory.join("host.json"))
        .arg("--memory-command")
        .arg(f.directory.join("memory-command.json"))
        .arg("--runtime")
        .arg(f.directory.join("runtime.json"))
        .output()
        .unwrap();
    assert!(!mixed.status.success());
    assert!(!f.directory.join("core.sqlite").exists());
    std::fs::write(f.directory.join("memory-command.json"), " ".repeat(65537)).unwrap();
    let large = std::process::Command::new(support::adapter_binary("arcs-host"))
        .arg("--config")
        .arg(f.directory.join("host.json"))
        .arg("--memory-command")
        .arg(f.directory.join("memory-command.json"))
        .output()
        .unwrap();
    assert!(!large.status.success());
    assert!(!f.directory.join("core.sqlite").exists());
}

#[test]
fn external_adapters_cannot_impersonate_the_memory_assessment_publisher() {
    let f = setup();
    let mut grant = f.read("a-grant.json");
    grant["adapter_id"] = json!(arcs::memory::PUBLISHER);
    f.write("a-grant.json", grant);
    assert!(Host::open(f.directory.join("host.json")).is_err());
    assert!(!f.directory.join("core.sqlite").exists());
}

#[test]
fn fresh_task_after_restart_observes_retraction_while_completed_work_is_not_repeated() {
    let f = setup();
    let (mut host, mut runtime) = f.open();
    let target = note(&host, "temporary", "project:public");
    let first = host
        .assess_memory(&command(&target, MemoryStatus::Active))
        .unwrap();
    finish(&mut host, &mut runtime);
    let original = invocation(&f);
    let mut retract = first.command;
    retract.expected_version_id = Some(first.version_id);
    retract.status = MemoryStatus::Retracted;
    host.assess_memory(&retract).unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let input = f.read("input-profile.json");
    let mut event = input["events"][0].clone();
    event["id"] = json!("second-input");
    event["reference"] = json!("second-reference");
    f.write("second-event.json", event);
    let queued = std::process::Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(f.directory.join("input-profile.json"))
        .arg(f.directory.join("input.sqlite"))
        .arg(f.directory.join("second-event.json"))
        .output()
        .unwrap();
    assert!(
        queued.status.success(),
        "{}",
        String::from_utf8_lossy(&queued.stderr)
    );
    let (mut host, mut runtime) = f.open();
    let deadline = Instant::now() + Duration::from_secs(20);
    while runtime.report(0).unwrap().ready_for_review < 2 {
        runtime.tick(&mut host).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let db = rusqlite::Connection::open(f.directory.join("a.sqlite")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT input FROM results ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let second: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(second["context"].as_array().unwrap().len(), 1);
    assert_eq!(original["context"].as_array().unwrap().len(), 2);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(f.count("a", "results"), 2);
}

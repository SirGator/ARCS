mod support;
use arcs::adapters::{AdapterId, CapabilityRef};
use arcs::core::{ActorType, SchemaId, SourceClass, TrustLevel};
use arcs::reasoning::{
    ContextSelection, ReasoningMethod, ReasoningRequestV2, ReasoningResources, ReasoningStatus,
};
use arcs_host::Host;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-roles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/roles");
        for name in [
            "input-profile.json",
            "input-grant.json",
            "solver-profile.json",
            "solver-grant.json",
            "model-profile.json",
            "model-grant.json",
        ] {
            std::fs::copy(source.join(name), directory.join(name)).unwrap();
        }
        let fixture = Self { directory };
        fixture.write("host.json",json!({"config_version":1,"database":"db/core.sqlite","schemas":[root.join("adapters/example-reasoner/schemas/task.schema.json"),root.join("adapters/example-reasoner/schemas/candidate.schema.json")],
            "adapters":[{"program":support::adapter_binary("arcs-json-input"),"profile":"input-profile.json","args":[{"path":"db/input.sqlite"}],"grant_file":"input-grant.json"},
                {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"solver-profile.json","args":[{"path":"db/solver.sqlite"}],"grant_file":"solver-grant.json"},
                {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"model-profile.json","args":[{"path":"db/model.sqlite"}],"grant_file":"model-grant.json"}]}));
        fixture
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn open(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn request(input: &arcs::core::Artifact, model: bool) -> ReasoningRequestV2 {
    ReasoningRequestV2 {
        request_id: format!("nearest:{}", input.version_id.0),
        capability: CapabilityRef::new(
            if model {
                "example.model-fixture"
            } else {
                "example.solver"
            },
            "solution.propose",
        ),
        method: if model {
            ReasoningMethod::Learned
        } else {
            ReasoningMethod::Solver
        },
        objective: "Find the nearest listed value".into(),
        context: vec![ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["values".into(), "target".into()],
        }],
        target_schema_id: SchemaId("arcs.solution_candidate.nearest.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: ReasoningResources {
            max_context_items: 1,
            max_context_bytes: 8192,
            max_output_bytes: 4096,
            max_candidates: 1,
            max_runtime_ms: 1000,
            max_iterations: if model { None } else { Some(4096) },
            max_output_tokens: None,
        },
    }
}

#[test]
fn oversized_candidate_is_rejected_before_result_commit() {
    let fixture = Fixture::new();
    let mut grant = fixture.read("solver-grant.json");
    grant["max_payload_bytes"] = json!(1);
    fixture.write("solver-grant.json", grant);
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let error = host.reason(request(&input, false)).unwrap_err();
    assert!(format!("{error:?}").contains("candidate exceeds grant payload limit"));
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|event| {
                event.artifact.schema_id.0 != "arcs.reasoning_result.v2"
                    && !event.artifact.artifact_type.ends_with("_candidate")
            })
    );
    assert_eq!(host.report().unwrap().world_revision, 0);
    host.shutdown().unwrap();
}

#[test]
fn external_input_and_two_independent_reasoners_preserve_trust_and_replay() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let inputs = host.poll_inputs().unwrap();
    assert_eq!(inputs.len(), 1);
    let input = &inputs[0];
    assert_eq!(input.trust.source_class, SourceClass::External);
    assert_eq!(host.report().unwrap().world_revision, 0);
    assert!(
        host.store()
            .current(input.subject.as_ref().unwrap(), &input.schema_id)
            .unwrap()
            .is_none()
    );
    assert!(host.poll_inputs().unwrap().is_empty());
    let solver = host.reason(request(input, false)).unwrap();
    let model = host.reason(request(input, true)).unwrap();
    assert_eq!(solver.status, ReasoningStatus::Solved);
    assert_eq!(solver.candidates[0].payload["value"], 2.0);
    assert_eq!(model.candidates[0].payload["value"], 1.0);
    assert_eq!(
        solver.candidates[0].created_by.actor_type,
        ActorType::Adapter
    );
    assert_eq!(solver.candidates[0].trust.level, TrustLevel::Medium);
    assert_eq!(model.candidates[0].trust.source_class, SourceClass::Model);
    assert_eq!(model.candidates[0].trust.level, TrustLevel::Low);
    let audit = host
        .store()
        .get(&solver.request_version_id)
        .unwrap()
        .unwrap();
    assert!(
        audit.payload["invocation"]["context"][0]["payload"]
            .get("private_note")
            .is_none()
    );
    assert!(
        host.execute_approved(
            &AdapterId("example.solver".into()),
            &solver.candidates[0].version_id
        )
        .is_err()
    );
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    assert!(host.poll_inputs().unwrap().is_empty());
    let replay = host.reason(request(input, false)).unwrap();
    assert_eq!(replay.candidates, solver.candidates);
    assert_eq!(replay.result_version_id, solver.result_version_id);
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    host.shutdown().unwrap();
}

#[test]
fn input_replay_conflicts_do_not_ack_or_change_world_state() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    // Simuliert einen nicht angekommenen ACK: dieselbe Outbox-Zeile wird erneut angeboten.
    rusqlite::Connection::open(fixture.directory.join("db/input.sqlite"))
        .unwrap()
        .execute("UPDATE events SET ack=0", [])
        .unwrap();
    let mut host = fixture.open();
    assert_eq!(host.poll_inputs().unwrap(), vec![input.clone()]);
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    host.shutdown().unwrap();
    drop(host);
    let db = rusqlite::Connection::open(fixture.directory.join("db/input.sqlite")).unwrap();
    let text: String = db
        .query_row("SELECT message FROM events", [], |row| row.get(0))
        .unwrap();
    let mut changed: Value = serde_json::from_str(&text).unwrap();
    changed["external_reference"] = json!("other-task");
    db.execute(
        "UPDATE events SET ack=0,message=?1",
        [serde_json::to_string(&changed).unwrap()],
    )
    .unwrap();
    drop(db);
    let mut host = fixture.open();
    assert!(host.poll_inputs().is_err());
    assert_eq!(host.report().unwrap().world_revision, 0);
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    let ack: bool = rusqlite::Connection::open(fixture.directory.join("db/input.sqlite"))
        .unwrap()
        .query_row("SELECT ack FROM events", [], |row| row.get(0))
        .unwrap();
    assert!(!ack);
}

#[test]
fn unsupported_methods_budgets_and_context_references_fail_before_dispatch() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let original = request(&input, false);
    let count = host.report().unwrap().stored_artifacts;
    for variant in 0..4 {
        let mut changed = original.clone();
        match variant {
            0 => changed.method = ReasoningMethod::Llm,
            1 => changed.budget.max_iterations = Some(5000),
            2 => changed.budget.max_output_bytes = 5000,
            _ => changed.context[0].payload_fields.push("absent".into()),
        }
        assert!(host.reason(changed).is_err());
        assert_eq!(host.report().unwrap().stored_artifacts, count);
    }
    let solved = host.reason(original.clone()).unwrap();
    let mut conflict = original;
    conflict.objective = "Different problem".into();
    assert!(host.reason(conflict).is_err());
    assert_eq!(solved.candidates.len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn model_origin_is_not_promoted_by_a_mathematical_solver() {
    let fixture = Fixture::new();
    let mut grant = fixture.read("input-grant.json");
    grant["producer_class"] = json!("model");
    grant["assigned_trust"] = json!("high");
    fixture.write("input-grant.json", grant);
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let result = host.reason(request(&input, false)).unwrap();
    assert_eq!(result.candidates[0].trust.source_class, SourceClass::Model);
    assert_eq!(result.candidates[0].trust.level, TrustLevel::Low);
    host.shutdown().unwrap();
}

#[test]
fn no_solution_and_iteration_exhaustion_are_persisted_results_without_candidates() {
    for empty in [false, true] {
        let fixture = Fixture::new();
        let mut profile = fixture.read("input-profile.json");
        if empty {
            profile["events"][0]["payload"]["values"] = json!([]);
            fixture.write("input-profile.json", profile);
        }
        let mut host = fixture.open();
        let input = host.poll_inputs().unwrap().remove(0);
        let mut task = request(&input, false);
        task.budget.max_iterations = Some(1);
        let result = host.reason(task.clone()).unwrap();
        assert!(result.candidates.is_empty());
        assert_eq!(
            result.status,
            if empty {
                ReasoningStatus::NoSolution
            } else {
                ReasoningStatus::BudgetExhausted
            }
        );
        assert_eq!(
            host.reason(task).unwrap().result_version_id,
            result.result_version_id
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn revoked_reasoning_permission_blocks_replay_and_model_methods_require_model_grants() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    host.reason(request(&input, false)).unwrap();
    host.shutdown().unwrap();
    drop(host);
    let mut grant = fixture.read("solver-grant.json");
    grant["granted_permissions"] = json!([]);
    fixture.write("solver-grant.json", grant);
    assert!(Host::open(fixture.directory.join("host.json")).is_err());
    let mut restored = fixture.read("solver-grant.json");
    restored["granted_permissions"] = json!(["reason.compute"]);
    fixture.write("solver-grant.json", restored);
    let mut grant = fixture.read("model-grant.json");
    grant["producer_class"] = json!("adapter");
    fixture.write("model-grant.json", grant);
    // Auch eine unveränderte Modellantwort darf keine Adapter-Herkunft erhalten.
    assert!(Host::open(fixture.directory.join("host.json")).is_err());
}

#[test]
fn lowering_a_reasoner_trust_grant_does_not_return_an_overtrusted_cached_candidate() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    host.reason(request(&input, false)).unwrap();
    let count = host.report().unwrap().stored_artifacts;
    host.shutdown().unwrap();
    drop(host);
    let mut grant = fixture.read("solver-grant.json");
    grant["assigned_trust"] = json!("low");
    fixture.write("solver-grant.json", grant);
    let mut host = fixture.open();
    assert!(host.reason(request(&input, false)).is_err());
    assert_eq!(host.report().unwrap().stored_artifacts, count);
    host.shutdown().unwrap();
}

#[cfg(unix)]
fn scripted_solver(fixture: &Fixture, timeout: bool) {
    let manifest = json!({"protocol_version":1,"adapter_id":"example.solver","adapter_version":"0.1.0","capabilities":[{
        "id":"solution.propose","contract":{"kind":"reason","emits":["arcs.solution_candidate.nearest.v1"]},"required_permissions":["reason.compute"]}]});
    let hello = json!({"protocol_version":1,"request_id":"stdio-1","message":{"type":"manifest","manifest":manifest}});
    let script = format!(
        "IFS= read -r line || exit 1\nprintf '%s\\n' '{}'\nIFS= read -r line || exit 1\n{}\n",
        hello,
        if timeout {
            "exec sleep 3"
        } else {
            "cat \"$1\"; printf '\\n'"
        }
    );
    std::fs::write(fixture.directory.join("solver.sh"), script).unwrap();
    let mut host = fixture.read("host.json");
    host["adapters"][1]["program"] = json!("/bin/sh");
    host["adapters"][1]["profile"] = Value::Null;
    host["adapters"][1]["args"] = json!([{"path":"solver.sh"},{"path":"response.json"}]);
    fixture.write("host.json", host);
}

#[cfg(unix)]
#[test]
fn malformed_correlations_control_schemas_and_out_of_context_candidates_are_rejected() {
    for variant in 0..4 {
        let fixture = Fixture::new();
        scripted_solver(&fixture, false);
        let mut host = fixture.open();
        let input = host.poll_inputs().unwrap().remove(0);
        let task = request(&input, false);
        let invocation = arcs::runtime::deterministic_invocation_id(
            arcs::runtime::InvocationKind::Reasoning,
            &["v2", "example.solver", "solution.propose", &task.request_id],
        );
        let mut response = json!({"invocation_id":invocation,"request_id":task.request_id,"status":"solved","candidates":[{
            "schema_id":"arcs.solution_candidate.nearest.v1","required_capabilities":[],"referenced_versions":[input.version_id],"payload":{"value":2,"distance":0.2}}],
            "trace":{"method":"solver","implementation":"test","version":"1","iterations":3,"output_tokens":null,"details":{}}});
        match variant {
            0 => response["invocation_id"] = json!("wrong"),
            1 => response["candidates"][0]["schema_id"] = json!("arcs.approval.v1"),
            2 => response["candidates"][0]["referenced_versions"] = json!(["not-selected"]),
            _ => {
                response["candidates"][0]["required_capabilities"] =
                    json!([{"adapter_id":"example.input","capability_id":"task.submit"}])
            }
        }
        fixture.write("response.json",json!({"protocol_version":1,"request_id":"stdio-2","message":{"type":"reasoning_result_v2","response":response}}));
        assert!(host.reason(task).is_err());
        assert!(
            !host
                .store()
                .committed_after(0)
                .unwrap()
                .iter()
                .any(|event| event.artifact.artifact_type.ends_with("_candidate"))
        );
        drop(host);
    }
}

#[cfg(unix)]
#[test]
fn reasoning_deadline_kills_a_stuck_process_without_synthesizing_a_result() {
    let fixture = Fixture::new();
    scripted_solver(&fixture, true);
    let mut host = fixture.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let mut task = request(&input, false);
    task.budget.max_runtime_ms = 20;
    let started = std::time::Instant::now();
    assert!(matches!(
        host.reason(task),
        Err(arcs_host::HostError::Reasoning(
            arcs::reasoning::ReasoningError::AdapterCall(arcs::adapters::AdapterCallError::Timeout)
        ))
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(host.report().unwrap().world_revision, 0);
    assert!(
        !host
            .store()
            .committed_after(0)
            .unwrap()
            .iter()
            .any(|event| event.artifact.schema_id.0 == "arcs.reasoning_result.v2")
    );
}

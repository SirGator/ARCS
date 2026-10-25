mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, VersionId};
use arcs_host::Host;
use arcs_host::task_runtime::goal::{GoalState, GoalTask};
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn assessor_peer(&self, mode: &str) {
        let script = r#"import json, sys
for line in sys.stdin:
    packet=json.loads(line); message=packet['message']; kind=message['type']
    if kind=='hello':
        reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'goal.assessor','adapter_version':'0.1.0','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.goal_assessment_candidate.v1']},'required_permissions':['reason.compute']}]}}
    elif kind=='reason_v2':
        inv=message['invocation']; goal=inv['constraints']['goal_runtime']; mode=sys.argv[1]
        evidence=list(goal['after'].values())
        if mode=='stale': evidence=list(goal['before'].values())
        if mode=='invented': evidence=['not-an-observation']
        payload={'goal_status':'achieved','effect':'unknown' if mode=='unknown' else 'success','detail':'external test assessment','evidence_version_ids':evidence}
        response={'invocation_id':inv['invocation_id'],'request_id':inv['request_id'],'status':'solved','candidates':[{'schema_id':inv['target_schema_id'],'required_capabilities':[],'referenced_versions':[c['version_id'] for c in inv['context']],'payload':payload}],'trace':{'method':inv['method'],'implementation':'assessment-peer','version':'1','iterations':1,'output_tokens':None,'details':{}}}
        if mode=='no_solution': response['status']='no_solution';response['candidates']=[]
        reply={'type':'reasoning_result_v2','response':response}
    elif kind=='shutdown': reply={'type':'bye'}
    else: raise RuntimeError(kind)
    print(json.dumps({'protocol_version':1,'request_id':packet['request_id'],'message':reply}),flush=True)
    if kind=='shutdown': break
"#;
        std::fs::write(self.directory.join("assessor.py"), script).unwrap();
        let mut config = self.read("host.json");
        config["adapters"][3] = json!({"program":"/usr/bin/python3","args":[{"path":"assessor.py"},mode],"grant_file":"assessor-grant.json","timeout_ms":15000});
        self.write("host.json", config);
    }
    fn new() -> Self {
        Self::example("goals")
    }
    fn example(example: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-goal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join(example);
        for file in std::fs::read_dir(&source).unwrap() {
            let file = file.unwrap();
            std::fs::copy(file.path(), directory.join(file.file_name())).unwrap();
        }
        let fixture = Self { directory };
        let mut host = fixture.read("host.json");
        host["database"] = json!(fixture.directory.join("core.sqlite"));
        for schema in host["schemas"].as_array_mut().unwrap() {
            *schema = json!(
                source
                    .join(schema.as_str().unwrap())
                    .canonicalize()
                    .unwrap()
            );
        }
        for adapter in host["adapters"].as_array_mut().unwrap() {
            let name = PathBuf::from(adapter["program"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            adapter["program"] = json!(support::adapter_binary(&name));
            let profile = adapter["profile"].as_str().unwrap();
            adapter["args"][0] =
                json!({"path":fixture.directory.join(profile.replace("-profile.json", ".sqlite"))});
        }
        fixture.write("host.json", host);
        let mut runtime = fixture.read("runtime.json");
        runtime["journal"] = json!(fixture.directory.join("runtime.sqlite"));
        runtime["max_jobs_per_tick"] = json!(1);
        runtime["interval_ms"] = json!(1);
        fixture.write("runtime.json", runtime);
        fixture
    }
    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn text(text: &str) -> Self {
        let fixture = Self::example("text-tasks");
        let mut profile = fixture.read("input-profile.json");
        profile["events"] = json!([{"id":"text-1","subject":"text-1","reference":"task:text-1","payload":{"raw_text":text}}]);
        fixture.write("input-profile.json", profile);
        fixture
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    fn host(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn runtime(&self, host: &Host) -> TaskRuntime {
        TaskRuntime::open(
            host,
            RuntimeConfig::load(self.directory.join("runtime.json")).unwrap(),
        )
        .unwrap()
    }
    fn db(&self, name: &str) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.join(format!("{name}.sqlite"))).unwrap()
    }
    fn manual(&self) {
        let mut config = self.read("runtime.json");
        config["routes"][0]["goal"]["approval"] = json!({"mode":"manual"});
        self.write("runtime.json", config);
    }
    fn with_observer(&self, temperature: bool) {
        let mut config = self.read("runtime.json");
        let source = if temperature {
            json!({"id":"ambient","capability":{"adapter_id":"demo.fan","capability_id":"temperature.observe"},"schema_id":"arcs.measurement.temperature.v1","payload_fields":["value","unit"]})
        } else {
            json!({"id":"observed-fan","capability":{"adapter_id":"demo.fan","capability_id":"fan.observe"},"schema_id":"arcs.state.fan.v1","payload_fields":["device_id","enabled","actuation_count"]})
        };
        config["routes"][0]["goal"]["sources"]
            .as_array_mut()
            .unwrap()
            .push(source);
        self.write("runtime.json", config);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn task(runtime: &TaskRuntime) -> GoalTask {
    runtime.goals_after(0, 100).unwrap().remove(0)
}
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    expected: impl Fn(GoalState) -> bool,
) -> GoalTask {
    for _ in 0..100 {
        runtime.tick(host).unwrap();
        let task = task(runtime);
        if expected(task.state) {
            return task;
        }
    }
    panic!("goal did not reach expected state: {:?}", task(runtime));
}
fn count(host: &Host, kind: &str) -> usize {
    host.store()
        .committed_after(0)
        .unwrap()
        .iter()
        .filter(|e| e.artifact.artifact_type == kind)
        .count()
}
fn human() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.simon".into(),
    }
}
fn actuations(fixture: &Fixture) -> i64 {
    fixture
        .db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn external_planner_and_assessor_complete_two_rounds_with_fresh_requests_and_observations() {
    let fixture = Fixture::new();
    fixture.with_observer(false);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(done.round, 2);
    assert_eq!(done.after.len(), 2);
    assert_eq!(actuations(&fixture), 2);
    assert_eq!(count(&host, "action"), 2);
    assert_eq!(count(&host, "outcome"), 2);
    assert_eq!(count(&host, "approval"), 2);
    let candidate = host
        .store()
        .get(done.candidate.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(candidate.created_by.id, "goal.planner");
    assert!(candidate.trust.level != arcs::core::TrustLevel::High);
    let outcome = host
        .store()
        .get(done.outcome.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(outcome.payload["verdict"], "success");
    let relations = arcs::store::ArtifactRelations::new(host.store())
        .outgoing(&outcome.version_id)
        .unwrap();
    for version in &done.after {
        assert!(relations.iter().any(|r| &r.to == version));
    }
    let report = runtime.report(0).unwrap();
    assert_eq!(report.completed_goals, 1);
    assert_eq!(report.pending_jobs, 0);
    assert_eq!(report.ready_for_review, 0);
    let total = host.report().unwrap().stored_artifacts;
    runtime.tick(&mut host).unwrap();
    assert_eq!(host.report().unwrap().stored_artifacts, total);
    host.shutdown().unwrap();
}

#[test]
fn action_verification_blocks_a_schema_valid_but_disallowed_payload_before_approval() {
    let fixture = Fixture::new();
    let mut schema = fixture.read("policy.schema.json");
    schema["properties"]["enabled"]["enum"] = json!([false]);
    fixture.write("policy.schema.json", schema);
    // The host configuration references copied policy files through canonical paths.
    let mut config = fixture.read("host.json");
    let last = config["schemas"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    *last = json!(fixture.directory.join("policy.schema.json"));
    fixture.write("host.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Blocked);
    assert!(done.verification.is_some());
    assert_eq!(count(&host, "approval"), 0);
    assert_eq!(count(&host, "action"), 0);
    assert_eq!(actuations(&fixture), 0);
    host.shutdown().unwrap();
}

#[test]
fn manual_review_requires_the_exact_candidate_and_an_authorized_actor_and_can_reject() {
    let fixture = Fixture::new();
    fixture.manual();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let waiting = until(&mut host, &mut runtime, |s| {
        s == GoalState::AwaitingApproval
    });
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(actuations(&fixture), 0);
    assert_eq!(count(&host, "approval"), 0);
    assert!(
        runtime
            .review_goal(
                &host,
                waiting.sequence,
                &VersionId("wrong".into()),
                ApprovalDecision::Approved,
                human(),
                "approve".into()
            )
            .is_err()
    );
    assert!(
        runtime
            .review_goal(
                &host,
                waiting.sequence,
                waiting.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::Model,
                    id: "model".into()
                },
                "approve".into()
            )
            .is_err()
    );
    runtime
        .review_goal(
            &host,
            waiting.sequence,
            waiting.candidate.as_ref().unwrap(),
            ApprovalDecision::Rejected,
            human(),
            "operator rejects this step".into(),
        )
        .unwrap();
    runtime.tick(&mut host).unwrap();
    assert_eq!(task(&runtime).state, GoalState::Rejected);
    assert_eq!(count(&host, "action"), 0);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    host.shutdown().unwrap();
}

#[test]
fn every_phase_resumes_after_restart_and_manual_approval_is_separate_for_each_round() {
    let fixture = Fixture::new();
    fixture.manual();
    fixture.with_observer(false);
    let mut approved = 0;
    for _ in 0..60 {
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        runtime.tick(&mut host).unwrap();
        let current = task(&runtime);
        if current.state == GoalState::AwaitingApproval {
            runtime
                .review_goal(
                    &host,
                    current.sequence,
                    current.candidate.as_ref().unwrap(),
                    ApprovalDecision::Approved,
                    human(),
                    "reviewed concrete step".into(),
                )
                .unwrap();
            approved += 1;
        }
        let done = current.state.terminal();
        host.shutdown().unwrap();
        if done {
            assert_eq!(current.state, GoalState::Completed);
            assert_eq!(approved, 2);
            assert_eq!(actuations(&fixture), 2);
            return;
        }
    }
    panic!("goal did not resume");
}

#[test]
fn interrupted_execution_checkpoint_replays_without_duplicate_effect_or_losing_fresh_observation() {
    let fixture = Fixture::new();
    fixture.with_observer(false);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    until(&mut host, &mut runtime, |s| s == GoalState::Observing);
    assert_eq!(actuations(&fixture), 1);
    let mut interrupted = serde_json::to_value(task(&runtime)).unwrap();
    interrupted["state"] = json!("executing");
    interrupted["action"] = Value::Null;
    interrupted["execution"] = Value::Null;
    assert!(interrupted["dispatch_cursor"].is_number());
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    fixture
        .db("runtime")
        .execute(
            "UPDATE runtime_jobs SET state='running',goal_json=?1",
            [interrupted.to_string()],
        )
        .unwrap();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert_eq!(actuations(&fixture), 1);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(actuations(&fixture), 2);
    assert_eq!(count(&host, "action"), 2);
    host.shutdown().unwrap();
}

#[test]
fn missing_post_action_observation_blocks_instead_of_using_the_execution_receipt_as_success() {
    let fixture = Fixture::new();
    fixture.with_observer(true);
    let mut config = fixture.read("runtime.json");
    config["routes"][0]["goal"]["max_wait_ticks"] = json!(3);
    fixture.write("runtime.json", config);
    let mut host_config = fixture.read("host.json");
    host_config["adapters"][1]["requests"] = json!([{"id":"seed-temp","capability_id":"temperature.read","input_schema_id":"arcs.request.temperature.v1","response_schema_id":"arcs.measurement.temperature.v1","subject":"temperature-01","payload":{"device_id":"temperature-01"}}]);
    fixture.write("host.json", host_config);
    let mut host = fixture.host();
    host.request("seed-temp").unwrap();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Blocked);
    assert!(done.execution.is_some());
    assert!(done.outcome.is_none());
    assert_eq!(actuations(&fixture), 1);
    assert_eq!(runtime.report(0).unwrap().completed_goals, 0);
    host.shutdown().unwrap();
}

#[test]
fn endless_replanning_hits_the_round_budget_and_can_be_cancelled_without_further_actions() {
    let fixture = Fixture::new();
    let mut profile = fixture.read("input-profile.json");
    profile["events"][0]["payload"]["minimum_actuations"] = json!(100);
    fixture.write("input-profile.json", profile);
    let mut config = fixture.read("runtime.json");
    config["routes"][0]["goal"]["max_rounds"] = json!(2);
    fixture.write("runtime.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::BudgetExhausted);
    assert_eq!(actuations(&fixture), 2);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    host.shutdown().unwrap();
    let fixture = Fixture::new();
    fixture.manual();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let waiting = until(&mut host, &mut runtime, |s| {
        s == GoalState::AwaitingApproval
    });
    runtime
        .cancel_goal(&host, waiting.sequence, "task withdrawn".into())
        .unwrap();
    runtime.tick(&mut host).unwrap();
    assert_eq!(task(&runtime).state, GoalState::Cancelled);
    assert_eq!(actuations(&fixture), 0);
    host.shutdown().unwrap();
}

#[test]
fn a_waiting_manual_goal_does_not_starve_other_jobs_or_observations_at_queue_capacity() {
    let fixture = Fixture::new();
    fixture.manual();
    fixture.with_observer(false);
    let mut profile = fixture.read("input-profile.json");
    let mut next = profile["events"][0].clone();
    next["id"] = json!("cycle-2");
    next["reference"] = json!("goal:cycle-2");
    profile["events"].as_array_mut().unwrap().push(next);
    fixture.write("input-profile.json", profile);
    let mut config = fixture.read("runtime.json");
    config["max_pending_jobs"] = json!(2);
    fixture.write("runtime.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    for _ in 0..20 {
        runtime.tick(&mut host).unwrap();
    }
    let goals = runtime.goals_after(0, 10).unwrap();
    assert_eq!(goals.len(), 2);
    assert!(goals.iter().all(|g| g.state == GoalState::AwaitingApproval));
    let first = &goals[0];
    runtime
        .review_goal(
            &host,
            first.sequence,
            first.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "run first only".into(),
        )
        .unwrap();
    for _ in 0..15 {
        runtime.tick(&mut host).unwrap();
    }
    let first = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(first.round, 2);
    assert_eq!(first.state, GoalState::AwaitingApproval);
    assert_eq!(actuations(&fixture), 1);
    host.shutdown().unwrap();
}

#[test]
fn unknown_or_unavailable_assessment_never_completes_a_goal() {
    for mode in ["unknown", "no_solution"] {
        let fixture = Fixture::new();
        fixture.assessor_peer(mode);
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        let done = until(&mut host, &mut runtime, GoalState::terminal);
        assert_eq!(done.state, GoalState::Blocked);
        assert_eq!(actuations(&fixture), 1);
        assert_eq!(runtime.report(0).unwrap().completed_goals, 0);
        if mode == "unknown" {
            let outcome = host
                .store()
                .get(done.outcome.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(outcome.payload["verdict"], "unknown");
        }
        host.shutdown().unwrap();
    }
}

#[test]
fn stale_or_invented_assessment_evidence_is_rejected_and_cannot_repeat_an_action_on_restart() {
    for mode in ["stale", "invented"] {
        let fixture = Fixture::new();
        fixture.assessor_peer(mode);
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        until(&mut host, &mut runtime, |s| s == GoalState::Assessing);
        assert!(runtime.tick(&mut host).is_err());
        assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
        assert_eq!(count(&host, "outcome"), 0);
        host.shutdown().unwrap();
        drop(runtime);
        drop(host);
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        assert!(runtime.tick(&mut host).is_err());
        assert_eq!(actuations(&fixture), 1);
        assert_eq!(runtime.report(0).unwrap().completed_goals, 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn cli_can_review_a_stopped_goal_and_resume_execution_then_cancel_the_next_step() {
    let fixture = Fixture::new();
    fixture.manual();
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(fixture.directory.join("host.json"))
            .arg("--runtime")
            .arg(fixture.directory.join("runtime.json"))
            .args(args)
            .current_dir(&fixture.directory)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let report = run(&["--ticks", "6"]);
    let goal = &report["goals"][0];
    assert_eq!(goal["state"], "awaiting_approval");
    fixture.write("review.json",json!({"command":"review","sequence":goal["sequence"],"candidate_version_id":goal["candidate"],"decision":"approved","operator_id":"simon","reason":"reviewed this concrete step"}));
    let report = run(&["--goal-command", "review.json"]);
    assert_eq!(report["goals"][0]["state"], "executing");
    assert_eq!(actuations(&fixture), 0);
    let report = run(&["--ticks", "12"]);
    assert_eq!(report["goals"][0]["state"], "awaiting_approval");
    assert_eq!(actuations(&fixture), 1);
    fixture.write(
        "cancel.json",
        json!({"command":"cancel","sequence":goal["sequence"],"reason":"cancel remaining work"}),
    );
    let report = run(&["--goal-command", "cancel.json"]);
    assert_eq!(report["goals"][0]["state"], "cancelled");
    assert_eq!(actuations(&fixture), 1);
}

#[test]
fn changing_an_operator_payload_bound_is_rejected_before_any_new_effect() {
    let fixture = Fixture::new();
    fixture.manual();
    let mut config = fixture.read("host.json");
    *config["schemas"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap() = json!(fixture.directory.join("policy.schema.json"));
    fixture.write("host.json", config);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    until(&mut host, &mut runtime, |s| {
        s == GoalState::AwaitingApproval
    });
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut schema = fixture.read("policy.schema.json");
    schema["properties"]["enabled"]["enum"] = json!([true]);
    fixture.write("policy.schema.json", schema);
    let error = match Host::open(fixture.directory.join("host.json")) {
        Ok(_) => panic!("changed schema accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SchemaDrift"));
    assert_eq!(actuations(&fixture), 0);
}

#[test]
fn a_committed_manual_decision_is_recovered_if_its_queue_checkpoint_was_interrupted() {
    let fixture = Fixture::new();
    fixture.manual();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let waiting = until(&mut host, &mut runtime, |s| {
        s == GoalState::AwaitingApproval
    });
    runtime
        .review_goal(
            &host,
            waiting.sequence,
            waiting.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "reviewed before crash".into(),
        )
        .unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    fixture
        .db("runtime")
        .execute(
            "UPDATE runtime_jobs SET state='awaiting_approval',goal_json=?1",
            [serde_json::to_string(&waiting).unwrap()],
        )
        .unwrap();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert_eq!(task(&runtime).state, GoalState::Executing);
    runtime.tick(&mut host).unwrap();
    assert_eq!(actuations(&fixture), 1);
    assert_eq!(count(&host, "approval"), 1);
    host.shutdown().unwrap();
}

#[test]
fn original_text_reaches_external_reasoners_and_completes_a_two_step_goal() {
    let text = "Schalte den Lüfter ein und dann wieder aus.";
    let fixture = Fixture::text(text);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(done.round, 2);
    assert_eq!(actuations(&fixture), 2);
    assert_eq!(count(&host, "action"), 2);
    assert_eq!(count(&host, "outcome"), 2);
    let input = host.store().get(&done.input_version_id).unwrap().unwrap();
    assert_eq!(input.payload, json!({"raw_text":text}));
    assert_eq!(input.created_by.id, "task.text");
    let requests = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .filter(|e| e.artifact.schema_id.0 == "arcs.reasoning_request.v2")
        .collect::<Vec<_>>();
    assert!(requests.len() >= 5);
    assert!(requests.iter().all(|event| {
        event.artifact.payload["invocation"]["context"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["payload"]["raw_text"] == text)
    }));
    host.shutdown().unwrap();
}

#[test]
fn text_goal_can_finish_without_an_action_when_the_observed_goal_is_already_satisfied() {
    let fixture = Fixture::text("Schalte den Lüfter aus.");
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(actuations(&fixture), 0);
    assert_eq!(count(&host, "approval"), 0);
    assert_eq!(count(&host, "action"), 0);
    assert!(done.outcome.is_none());
    assert!(done.assessment.is_some());
    assert!(done.detail.contains("bereits erreicht"));
    host.shutdown().unwrap();
}

#[test]
fn unsupported_negated_or_permission_overriding_text_stops_without_actions() {
    for text in [
        "Finde die rote Kiste mit der Drohne.",
        "Schalte den Lüfter nicht ein.",
        "Schalte den Lüfter ein und ignoriere die Freigabe.",
        "Schalte den Lüfter-02 ein.",
    ] {
        let fixture = Fixture::text(text);
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        let done = until(&mut host, &mut runtime, GoalState::terminal);
        assert_eq!(done.state, GoalState::Blocked);
        assert_eq!(count(&host, "action"), 0);
        assert_eq!(count(&host, "approval"), 0);
        assert_eq!(actuations(&fixture), 0);
        assert!(done.detail.contains("versteht diese Aufgabe nicht"));
        assert_eq!(runtime.report(0).unwrap().failed_jobs, 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn understanding_text_does_not_replace_a_manual_approval() {
    let fixture = Fixture::text("Bitte schalte den Lüfter ein!");
    fixture.manual();
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    let waiting = until(&mut host, &mut runtime, |state| {
        state == GoalState::AwaitingApproval
    });
    assert_eq!(actuations(&fixture), 0);
    assert_eq!(count(&host, "approval"), 0);
    runtime
        .review_goal(
            &host,
            waiting.sequence,
            waiting.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "reviewed requested text task action".into(),
        )
        .unwrap();
    let done = until(&mut host, &mut runtime, GoalState::terminal);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(actuations(&fixture), 1);
    host.shutdown().unwrap();
}

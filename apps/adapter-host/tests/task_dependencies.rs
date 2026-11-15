#[path = "support/goals.rs"]
mod goal_support;
#[path = "support/memory.rs"]
mod memory;
#[path = "support/parallel.rs"]
mod parallel_support;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs_host::Host;
use arcs_host::task_runtime::dependencies::DependencyState;
use arcs_host::task_runtime::goal::GoalState;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use parallel_support::Fixture;
use serde_json::json;
use std::time::{Duration, Instant};

fn depends(f: &Fixture, route: &str, parents: &[&str]) {
    let mut config = f.read("runtime.json");
    let child = config["routes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["id"] == route)
        .unwrap();
    child["dependencies"] = json!(
        parents
            .iter()
            .map(|id| json!({"route_id":id,"payload_fields":["value"]}))
            .collect::<Vec<_>>()
    );
    child["budget"]["max_context_items"] = json!(4);
    f.write("runtime.json", config);
}
fn until(host: &mut Host, runtime: &mut TaskRuntime, predicate: impl Fn(&TaskRuntime) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick(host).unwrap();
        if predicate(runtime) {
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
fn fanout(a: &str, b: &str, join: &str) -> Fixture {
    let f = Fixture::new();
    for (id, mode) in [("a", a), ("b", b), ("join", join)] {
        f.peer(id, mode, false);
        f.route(id, id);
    }
    depends(&f, "join", &["a", "b"]);
    f
}
fn assert_no_authority(host: &Host) {
    assert!(host.store().committed_after(0).unwrap().iter().all(|e| {
        !["action", "execution", "approval"].contains(&e.artifact.artifact_type.as_str())
    }));
}

#[test]
fn parallel_branches_join_only_after_both_results_and_pass_only_selected_fields() {
    let f = fanout("gate", "gate", "join");
    let mut config = f.read("runtime.json");
    // Dependents may occur before their prerequisites in the configuration.
    config["routes"].as_array_mut().unwrap().rotate_right(1);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    assert!(!f.started("join"));
    assert_eq!(runtime.report(0).unwrap().waiting_dependency, 1);
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert!(!f.started("join"));
    f.release("a");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    let receipt = runtime.dependencies_after(0, 1).unwrap().remove(0);
    assert_eq!(receipt.state, DependencyState::Ready);
    assert_eq!(receipt.bindings.len(), 2);
    assert!(
        receipt
            .bindings
            .iter()
            .all(|binding| binding.context[0].payload_fields == ["value"])
    );
    let done = runtime
        .completed_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|d| d.route_id == "join")
        .unwrap();
    let candidate = host
        .store()
        .get(&done.candidate_version_ids[0])
        .unwrap()
        .unwrap();
    assert_eq!(candidate.payload["value"], 4);
    assert_eq!(candidate.trust.level, arcs::core::TrustLevel::Low);
    let request = host.store().get(&done.request_version_id).unwrap().unwrap();
    let context = request.payload["invocation"]["context"].as_array().unwrap();
    let bindings = request.payload["invocation"]["constraints"]["task_dependencies"]
        .as_array()
        .unwrap();
    assert_eq!(
        bindings
            .iter()
            .map(|binding| binding["route_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(context.len(), 3);
    assert!(
        context
            .iter()
            .all(|item| item["payload"].get("private_note").is_none())
    );
    for item in context
        .iter()
        .filter(|item| item["schema_id"] == "arcs.solution_candidate.nearest.v1")
    {
        assert_eq!(item["payload"].as_object().unwrap().len(), 1);
    }
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn failed_prerequisites_block_transitive_dependents_without_stopping_other_work() {
    for mode in ["fail", "invalid", "no_solution"] {
        let f = fanout(mode, "normal", "join");
        f.peer("tail", "join", false);
        f.route("tail", "tail");
        depends(&f, "tail", &["join"]);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            let report = r.report(0).unwrap();
            report.blocked_dependencies == 2 && report.pending_jobs == 0
        });
        assert!(!f.started("join"));
        assert!(!f.started("tail"));
        assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
        assert_eq!(runtime.report(0).unwrap().blocked_goals, 0);
        assert!(
            runtime
                .dependencies_after(0, 10)
                .unwrap()
                .iter()
                .all(|r| r.state == DependencyState::Blocked)
        );
        assert_eq!(f.count("b", "results"), 1);
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}

#[test]
fn joins_recover_a_lost_response_with_identical_bindings_and_frozen_memory() {
    let f = fanout("normal", "normal", "gate");
    let mut config = f.read("runtime.json");
    config["routes"][2]["memory"] = memory::policy("project:join");
    f.write("runtime.json", config);
    // Grant must cover the configured mandatory context plus recalled memory.
    let mut grant = f.read("join-grant.json");
    grant["reasoning_limits_v2"]["resources"]["max_context_items"] = json!(8);
    f.write("join-grant.json", grant);
    let (mut host, mut runtime) = f.open();
    memory::append(&host, "old-join-experience", "project:join");
    until(&mut host, &mut runtime, |_| f.started("join"));
    let saved = serde_json::to_value(runtime.dependencies_after(0, 10).unwrap()).unwrap();
    assert_eq!(f.count("join", "results"), 1);
    memory::append(&host, "experience-after-dispatch", "project:join");
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("join");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    assert_eq!(
        serde_json::to_value(runtime.dependencies_after(0, 10).unwrap()).unwrap(),
        saved
    );
    assert_eq!(f.count("join", "results"), 1);
    assert_eq!(f.count("join", "attempts"), 2);
    assert_eq!(memory::snapshots(&host).len(), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn separate_inputs_keep_separate_dependency_instances_and_result_contexts() {
    let f = fanout("normal", "normal", "join");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    f.write("second.json",json!({"id":"second","subject":"another-project","reference":"task:second","payload":{"values":[3,7],"target":6}}));
    let output = std::process::Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(f.directory.join("input-profile.json"))
        .arg(f.directory.join("input.sqlite"))
        .arg(f.directory.join("second.json"))
        .output()
        .unwrap();
    assert!(output.status.success());
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 6
    });
    let receipts = runtime.dependencies_after(0, 10).unwrap();
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[0].input_version_id, receipts[1].input_version_id);
    for receipt in &receipts {
        let done = runtime.completed_after(0, 10).unwrap();
        for binding in &receipt.bindings {
            assert_eq!(
                done.iter()
                    .find(|d| d.sequence == binding.sequence)
                    .unwrap()
                    .input_version_id,
                receipt.input_version_id
            );
        }
    }
    let joins: Vec<_> = runtime
        .completed_after(0, 10)
        .unwrap()
        .into_iter()
        .filter(|d| d.route_id == "join")
        .collect();
    let values: Vec<_> = joins
        .iter()
        .map(|d| {
            host.store()
                .get(&d.candidate_version_ids[0])
                .unwrap()
                .unwrap()
                .payload["value"]
                .clone()
        })
        .collect();
    assert_eq!(values, vec![json!(4), json!(14)]);
    host.shutdown().unwrap();
}

#[test]
fn binding_commit_failure_does_not_start_a_join_or_repeat_completed_branches() {
    let f = fanout("normal", "normal", "join");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_join BEFORE UPDATE ON runtime_job_joins WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'injected join commit failure'); END;").unwrap();
    assert!(runtime.tick(&mut host).is_err());
    assert!(!f.started("join"));
    assert_eq!(
        runtime.dependencies_after(0, 1).unwrap()[0].state,
        DependencyState::WaitingDependency
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db.execute_batch("DROP TRIGGER fail_join;").unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(f.count("join", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn malformed_or_cyclic_graphs_are_rejected_before_any_runtime_job_is_created() {
    let f = fanout("normal", "normal", "join");
    f.peer("observer", "normal", true);
    let original = f.read("runtime.json");
    let mut variants = vec![];
    for dependencies in [
        json!([{"route_id":"missing","payload_fields":["value"]}]),
        json!([{"route_id":"join","payload_fields":["value"]}]),
        json!([{"route_id":"a","payload_fields":[]}]),
        json!([{"route_id":"a","payload_fields":["value","value"]}]),
        json!([{"route_id":"a","payload_fields":["value"]},{"route_id":"a","payload_fields":["value"]}]),
    ] {
        let mut config = original.clone();
        config["routes"][2]["dependencies"] = dependencies;
        variants.push(config);
    }
    let mut cycle = original.clone();
    cycle["routes"][0]["dependencies"] = json!([{"route_id":"join","payload_fields":["value"]}]);
    cycle["routes"][0]["budget"]["max_context_items"] = json!(4);
    variants.push(cycle);
    let mut budget = original.clone();
    budget["routes"][2]["budget"]["max_context_items"] = json!(2);
    variants.push(budget);
    let mut reserved = original.clone();
    reserved["routes"][2]["constraints"] = json!({"task_dependencies":[]});
    variants.push(reserved);
    let mut source = original.clone();
    source["routes"][2]["source"] = json!({"adapter_id":"observer","capability_id":"status"});
    variants.push(source);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    for config in variants {
        f.write("runtime.json", config);
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err()
        );
        assert!(!f.directory.join("runtime.sqlite").exists());
    }
    host.shutdown().unwrap();
}

#[test]
fn missing_selected_fields_block_the_join_but_multiple_candidates_can_be_bound() {
    for missing in [false, true] {
        let f = Fixture::new();
        f.peer("a", "many", false);
        f.route("a", "a");
        f.peer("join", "join", false);
        f.route("join", "join");
        depends(&f, "join", &["a"]);
        let mut config = f.read("runtime.json");
        config["routes"][0]["budget"]["max_candidates"] = json!(2);
        if missing {
            config["routes"][1]["dependencies"][0]["payload_fields"] = json!(["absent"]);
        }
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            if missing {
                r.report(0).unwrap().blocked_dependencies == 1
            } else {
                r.report(0).unwrap().ready_for_review == 2
            }
        });
        if missing {
            assert!(!f.started("join"));
        } else {
            assert_eq!(
                runtime.dependencies_after(0, 1).unwrap()[0].bindings[0]
                    .context
                    .len(),
                2
            );
        }
        host.shutdown().unwrap();
    }
}

fn goal_chain() -> goal_support::Fixture {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut config = f.read("runtime.json");
    let mut child = config["routes"][0].clone();
    child["id"] = json!("child-goal");
    child["dependencies"] =
        json!([{"route_id":config["routes"][0]["id"],"payload_fields":["goal_status","effect"]}]);
    config["routes"].as_array_mut().unwrap().push(child);
    config["max_pending_jobs"] = json!(2);
    f.write("runtime.json", config);
    f
}
fn human() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.test".into(),
    }
}

#[test]
fn a_dependent_goal_waits_for_confirmed_parent_completion_and_needs_its_own_approval() {
    let f = goal_chain();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..2 {
        until(&mut host, &mut runtime, |r| {
            r.goals_after(0, 10)
                .unwrap()
                .iter()
                .any(|g| g.route_id != "child-goal" && g.state == GoalState::AwaitingApproval)
        });
        assert_eq!(runtime.goals_after(0, 10).unwrap().len(), 1);
        let goal = runtime.goals_after(0, 10).unwrap().remove(0);
        runtime
            .review_goal(
                &host,
                goal.sequence,
                goal.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "approve parent round".into(),
            )
            .unwrap();
    }
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.route_id == "child-goal" && g.state == GoalState::AwaitingApproval)
    });
    let goals = runtime.goals_after(0, 10).unwrap();
    let parent = goals.iter().find(|g| g.route_id != "child-goal").unwrap();
    let child = goals.iter().find(|g| g.route_id == "child-goal").unwrap();
    assert_eq!(parent.state, GoalState::Completed);
    let candidate = host
        .store()
        .get(child.candidate.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        candidate
            .provenance
            .unwrap()
            .parents
            .contains(&parent.assessment.as_ref().unwrap().0)
    );
    assert!(child.approval.is_none());
    assert!(
        runtime
            .review_goal(
                &host,
                child.sequence,
                parent.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "reuse parent's review".into()
            )
            .is_err()
    );
    runtime
        .review_goal(
            &host,
            child.sequence,
            child.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            human(),
            "independent child review".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.route_id == "child-goal" && g.state == GoalState::Observing)
    });
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    runtime
        .cancel_goal(
            &host,
            child.sequence,
            "stop after verified third effect".into(),
        )
        .unwrap();
    host.shutdown().unwrap();
}

#[test]
fn a_rejected_goal_blocks_its_child_without_counting_it_as_an_executed_goal() {
    let f = goal_chain();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().awaiting_approval == 1
    });
    let parent = runtime.goals_after(0, 10).unwrap().remove(0);
    runtime
        .review_goal(
            &host,
            parent.sequence,
            parent.candidate.as_ref().unwrap(),
            ApprovalDecision::Rejected,
            human(),
            "reject parent".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().blocked_dependencies == 1
    });
    assert_eq!(runtime.goals_after(0, 10).unwrap().len(), 1);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert_eq!(runtime.report(0).unwrap().blocked_goals, 1);
    assert!(runtime.dependencies_after(0, 0).is_err());
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    host.shutdown().unwrap();
}

#[test]
fn an_action_candidate_cannot_drop_its_configured_dependency_evidence() {
    let f = goal_chain();
    let script = r#"import json,sys
for line in sys.stdin:
 p=json.loads(line);m=p['message'];kind=m['type']
 if kind=='hello':reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'goal.planner','adapter_version':'1','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.fan_candidate.v1']},'required_permissions':['reason.compute']}]}}
 elif kind=='reason_v2':
  inv=m['invocation'];ref=inv['constraints']['goal_runtime']['before']['fan-state']
  state=next(item['payload'] for item in inv['context'] if item['version_id']==ref)
  refs=[item['version_id'] for item in inv['context'] if item['schema_id']!='arcs.goal_assessment_candidate.v1']
  reply={'type':'reasoning_result_v2','response':{'invocation_id':inv['invocation_id'],'request_id':inv['request_id'],'status':'solved','candidates':[{'schema_id':inv['target_schema_id'],'required_capabilities':[inv['allowed_capabilities'][0]],'referenced_versions':refs,'payload':{'device_id':state['device_id'],'enabled':not state['enabled']}}],'trace':{'method':inv['method'],'implementation':'omitted-dependency-test','version':'1','iterations':1,'output_tokens':None,'details':{}}}}
 elif kind=='shutdown':reply={'type':'bye'}
 else:raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
"#;
    std::fs::write(f.directory.join("planner.py"), script).unwrap();
    let mut config = f.read("host.json");
    config["adapters"][2] = json!({"program":"/usr/bin/python3","args":[{"path":"planner.py"}],"grant_file":"planner-grant.json","timeout_ms":15000});
    f.write("host.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..2 {
        until(&mut host, &mut runtime, |r| {
            r.goals_after(0, 10)
                .unwrap()
                .iter()
                .any(|g| g.route_id != "child-goal" && g.state == GoalState::AwaitingApproval)
        });
        let goal = runtime.goals_after(0, 10).unwrap().remove(0);
        runtime
            .review_goal(
                &host,
                goal.sequence,
                goal.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                human(),
                "approve parent round".into(),
            )
            .unwrap();
    }
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.route_id == "child-goal" && g.state == GoalState::Blocked)
    });
    assert_eq!(runtime.report(0).unwrap().awaiting_approval, 0);
    assert_eq!(runtime.report(0).unwrap().blocked_dependencies, 0);
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .filter(|e| e.artifact.artifact_type == "approval")
            .count(),
        2
    );
    host.shutdown().unwrap();
}

#[path = "support/goals.rs"]
mod goal_support;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs_host::{
    Host,
    task_runtime::{
        RuntimeConfig, TaskRuntime,
        goal::{GoalState, GoalTask},
        planning::PlanState,
    },
};
use goal_support::Fixture;
use serde_json::json;
use std::time::{Duration, Instant};

fn fixture() -> Fixture {
    goal_support::plan_fixture()
}

fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(runtime) {
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
fn awaiting(host: &mut Host, runtime: &mut TaskRuntime) -> GoalTask {
    until(host, runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.state == GoalState::AwaitingApproval)
    });
    runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.state == GoalState::AwaitingApproval)
        .unwrap()
}
fn review(host: &Host, runtime: &mut TaskRuntime, goal: &GoalTask, decision: ApprovalDecision) {
    runtime
        .review_goal(
            host,
            goal.sequence,
            goal.candidate.as_ref().unwrap(),
            decision,
            Actor {
                actor_type: ActorType::Human,
                id: "operator.simon".into(),
            },
            "reviewed this concrete simulated-device action".into(),
        )
        .unwrap();
}
fn acts(f: &Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap()
}
fn count(host: &Host, schema: &str) -> usize {
    host.store()
        .committed_after(0)
        .unwrap()
        .iter()
        .filter(|e| e.artifact.schema_id.0 == schema)
        .count()
}

#[test]
fn observations_from_before_the_generated_goal_started_do_not_satisfy_initial_acquisition() {
    let f = fixture();
    let mut original = f.host();
    let manifest = original
        .registry()
        .authorized_capability(
            &arcs::adapters::AdapterId("demo.fan".into()),
            &arcs::adapters::CapabilityId("fan.observe".into()),
        )
        .unwrap()
        .0
        .manifest()
        .clone();
    original.shutdown().unwrap();
    drop(original);
    // The only observation arrives after the user input but before the plan
    // can create its goal. No new observation follows the acquisition barrier.
    let script = format!(
        "import json,sys\nmanifest=json.loads({:?})\n",
        serde_json::to_string(&manifest).unwrap()
    ) + r#"
seen=False
for line in sys.stdin:
 p=json.loads(line);m=p['message'];kind=m['type']
 if kind=='hello':reply={'type':'manifest','manifest':manifest}
 elif kind=='poll':
  if seen:reply={'type':'idle'}
  else:
   seen=True;reply={'type':'observation','message_id':'old','observation':{'capability_id':'fan.observe','external_subject':'fan-01','external_reference':'old','payload':{'device_id':'fan-01','enabled':False,'actuation_count':0}}}
 elif kind=='ack':reply={'type':'acknowledged','message_id':m['message_id']}
 elif kind=='shutdown':reply={'type':'bye'}
 else:raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
"#;
    std::fs::write(f.directory.join("old-observer.py"), script).unwrap();
    let mut host_config = f.read("host.json");
    host_config["adapters"][1] = json!({"program":"/usr/bin/python3","args":[{"path":"old-observer.py"}],"grant_file":"fan-grant.json","timeout_ms":15000});
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["sources"] = json!([{"id":"observed-fan","capability":{"adapter_id":"demo.fan","capability_id":"fan.observe"},"schema_id":"arcs.state.fan.v1","payload_fields":["device_id","enabled","actuation_count"]}]);
    config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["max_wait_ticks"] = json!(3);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().stopped_plans == 1
    });
    let goal = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(goal.state, GoalState::Blocked);
    assert!(goal.before.is_empty());
    let old = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .find(|e| {
            e.artifact.created_by.id == "demo.fan" && e.artifact.schema_id.0 == "arcs.state.fan.v1"
        })
        .unwrap();
    assert!(old.sequence <= goal.observation_cursor);
    let input_sequence = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .find(|e| e.artifact.version_id == goal.input_version_id)
        .unwrap()
        .sequence;
    assert!(old.sequence > input_sequence);
    assert_eq!(acts(&f), 0);
    assert_eq!(count(&host, "arcs.approval.v1"), 0);
    host.shutdown().unwrap();
}

#[test]
fn selected_goal_template_requires_manual_approval_and_completes_only_after_effect_assessment() {
    let f = fixture();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = awaiting(&mut host, &mut runtime);
    assert_eq!(goal.route_id, "p1.device");
    assert_eq!(acts(&f), 0);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Running
    );
    assert_eq!(count(&host, "arcs.approval.v1"), 0);
    let plan = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone()
        .unwrap();
    review(&host, &mut runtime, &goal, ApprovalDecision::Approved);
    assert_eq!(acts(&f), 0);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    let done = runtime.goals_after(0, 10).unwrap().remove(0);
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(acts(&f), 1);
    assert_eq!(count(&host, "arcs.approval.v1"), 1);
    assert_eq!(runtime.report(0).unwrap().completed_goals, 1);
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert!(done.assessment.is_some() && done.outcome.is_some());
    assert_eq!(
        host.store().get(&plan).unwrap().unwrap().trust.level,
        arcs::core::TrustLevel::Low
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..6 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(acts(&f), 1);
    assert_eq!(count(&host, "arcs.plan.v1"), 1);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0]
            .plan_version_id
            .as_ref(),
        Some(&plan)
    );
    host.shutdown().unwrap();
}

#[test]
fn rejecting_or_cancelling_a_plan_at_approval_cannot_dispatch_or_be_revived_by_late_reviews() {
    for cancel in [false, true] {
        let f = fixture();
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        let goal = awaiting(&mut host, &mut runtime);
        if cancel {
            runtime
                .cancel_plan(&mut host, 1, "cancel unapproved device work".into())
                .unwrap();
            assert_eq!(
                runtime.goals_after(0, 10).unwrap()[0].state,
                GoalState::Cancelled
            );
            assert!(
                runtime
                    .review_goal(
                        &host,
                        goal.sequence,
                        goal.candidate.as_ref().unwrap(),
                        ApprovalDecision::Approved,
                        Actor {
                            actor_type: ActorType::Human,
                            id: "late.operator".into()
                        },
                        "late approval".into()
                    )
                    .is_err()
            );
        } else {
            review(&host, &mut runtime, &goal, ApprovalDecision::Rejected);
        }
        until(&mut host, &mut runtime, |r| {
            r.report(0).unwrap().stopped_plans == 1
        });
        assert_eq!(acts(&f), 0);
        assert_eq!(runtime.report(0).unwrap().awaiting_approval, 0);
        host.shutdown().unwrap();
        drop(runtime);
        drop(host);
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        for _ in 0..6 {
            runtime.tick(&mut host).unwrap();
        }
        assert_eq!(acts(&f), 0);
        assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn goal_templates_reject_policy_approval_missing_action_limits_and_ungranted_actions_before_journaling()
 {
    for case in ["policy", "missing_limit", "revoked_action"] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        match case {
            "policy" => {
                config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["approval"] =
                    json!({"mode":"policy","policy_id":"cannot-be-selected-automatically"})
            }
            "missing_limit" => {
                config["routes"][0]["plan"]
                    .as_object_mut()
                    .unwrap()
                    .remove("max_total_actions");
            }
            "revoked_action" => {
                let mut grant = f.read("fan-grant.json");
                grant["enabled_capabilities"] = json!(["fan.read"]);
                f.write("fan-grant.json", grant);
            }
            _ => unreachable!(),
        }
        f.write("runtime.json", config);
        let mut host = f.host();
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err(),
            "{case}"
        );
        assert!(!f.directory.join("runtime.sqlite").exists());
        assert_eq!(acts(&f), 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn compile_counts_all_goal_rounds_assessments_initial_checks_and_declared_actions() {
    for case in [
        "round_runtime",
        "assessment_runtime",
        "initial_check",
        "actions",
        "repeated_template",
    ] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        match case {
            "round_runtime" => config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(30000),
            "assessment_runtime" => {
                config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(45000)
            }
            "initial_check" => {
                config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["initial_assessment"] =
                    json!(true);
                config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(60000);
            }
            "actions" => config["routes"][0]["plan"]["max_total_actions"] = json!(1),
            "repeated_template" => {
                config["routes"][0]["plan"]["max_total_actions"] = json!(3);
                f.write("planner.json",json!({"proposal":{"steps":[{"id":"one","template":"switch"},{"id":"two","template":"switch"}]}}));
            }
            _ => unreachable!(),
        }
        f.write("runtime.json", config);
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        until(&mut host, &mut runtime, |r| {
            r.report(0).unwrap().stopped_plans == 1
        });
        assert_eq!(
            runtime.plans_after(0, 1).unwrap()[0].state,
            PlanState::Rejected,
            "{case}"
        );
        assert!(runtime.goals_after(0, 10).unwrap().is_empty());
        assert_eq!(acts(&f), 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn a_completed_goal_exports_its_assessment_to_a_dependent_reasoning_step() {
    let f = fixture();
    // A separate Rules peer consumes only the selected assessment fields.
    // The actual goal assessor remains the bundled adapter.
    std::fs::write(
        f.directory.join("reporter.py"),
        include_str!("support/planner_peer.py"),
    )
    .unwrap();
    f.write("reporter.json",json!({"adapter_id":"reporter","target_schema_id":"arcs.goal_assessment_candidate.v1","context_namespace":"task_dependencies","iterations":1,"copy_assessment":true,"proposal":{"goal_status":"achieved","effect":"unknown","detail":"dependency report","evidence_version_ids":[]}}));
    let mut grant = f.read("assessor-grant.json");
    grant["adapter_id"] = json!("reporter");
    f.write("reporter-grant.json", grant);
    let mut host_config = f.read("host.json");
    host_config["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"reporter.py"},{"path":"reporter.json"}],"grant_file":"reporter-grant.json","timeout_ms":15000}));
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    let mut report = config["routes"][0]["plan"]["templates"][0]["route"].clone();
    report.as_object_mut().unwrap().remove("goal");
    report["id"] = json!("report");
    report["reasoner"]["adapter_id"] = json!("reporter");
    report["allowed_capabilities"] = json!([]);
    report["target_schema_id"] = json!("arcs.goal_assessment_candidate.v1");
    config["routes"][0]["plan"]["templates"]
        .as_array_mut()
        .unwrap()
        .push(json!({"route":report,"export_fields":["detail"]}));
    f.write("runtime.json", config);
    f.write("planner.json",json!({"proposal":{"steps":[{"id":"device","template":"switch"},{"id":"report","template":"report","dependencies":[{"step_id":"device","payload_fields":["goal_status","effect"]}]}]}}));
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = awaiting(&mut host, &mut runtime);
    assert_eq!(
        f.db("reporter")
            .query_row("SELECT COUNT(*) FROM results", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    review(&host, &mut runtime, &goal, ApprovalDecision::Approved);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(acts(&f), 1);
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 2);
    assert_eq!(
        runtime.dependencies_after(0, 10).unwrap()[0].bindings[0].context[0].version_id,
        runtime.goals_after(0, 10).unwrap()[0]
            .assessment
            .clone()
            .unwrap()
    );
    host.shutdown().unwrap();
}

#[test]
fn restarted_approval_and_interrupted_execution_checkpoint_keep_one_effect() {
    let f = fixture();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = awaiting(&mut host, &mut runtime);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    assert_eq!(
        runtime.goals_after(0, 10).unwrap()[0].candidate,
        goal.candidate
    );
    review(&host, &mut runtime, &goal, ApprovalDecision::Approved);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10).unwrap()[0].state == GoalState::Observing
    });
    assert_eq!(acts(&f), 1);
    let mut checkpoint =
        serde_json::to_value(runtime.goals_after(0, 10).unwrap()[0].clone()).unwrap();
    checkpoint["state"] = json!("executing");
    checkpoint["action"] = json!(null);
    checkpoint["execution"] = json!(null);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.db("runtime")
        .execute(
            "UPDATE runtime_jobs SET state='running',goal_json=?2 WHERE sequence=?1",
            rusqlite::params![goal.sequence as i64, checkpoint.to_string()],
        )
        .unwrap();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(acts(&f), 1);
    assert_eq!(count(&host, "arcs.approval.v1"), 1);
    assert_eq!(count(&host, "arcs.execution_result.fan.v1"), 1);
    assert_eq!(
        f.db("planner")
            .query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn template_verification_still_blocks_before_approval_and_inconclusive_assessment_blocks_completion()
 {
    for verification in [true, false] {
        let f = fixture();
        if verification {
            let mut policy = f.read("policy.schema.json");
            policy["properties"]["enabled"]["enum"] = json!([false]);
            f.write("strict-policy.json", policy);
            let mut host_config = f.read("host.json");
            let schemas = host_config["schemas"].as_array_mut().unwrap();
            *schemas.last_mut().unwrap() = json!(f.directory.join("strict-policy.json"));
            f.write("host.json", host_config);
        } else {
            f.assessor_peer("unknown");
        }
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        if !verification {
            let goal = awaiting(&mut host, &mut runtime);
            review(&host, &mut runtime, &goal, ApprovalDecision::Approved);
        }
        until(&mut host, &mut runtime, |r| {
            r.report(0).unwrap().stopped_plans == 1
        });
        assert_eq!(
            runtime.plans_after(0, 1).unwrap()[0].state,
            PlanState::Blocked
        );
        assert_eq!(acts(&f), if verification { 0 } else { 1 });
        assert_eq!(runtime.report(0).unwrap().review_plans, 0);
        assert_eq!(
            count(&host, "arcs.approval.v1"),
            if verification { 0 } else { 1 }
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn aggregate_token_limits_include_model_assessments_and_initial_checks() {
    for initial in [false, true] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        let template = &mut config["routes"][0]["plan"]["templates"][0]["route"];
        template["goal"]["initial_assessment"] = json!(initial);
        template["goal"]["assessment"]["method"] = json!("learned");
        template["goal"]["assessment"]["budget"]["max_output_tokens"] = json!(32);
        config["routes"][0]["plan"]["max_total_output_tokens"] =
            json!(if initial { 96 } else { 32 });
        f.write("runtime.json", config);
        f.assessor_peer("unknown");
        let mut grant = f.read("assessor-grant.json");
        grant["producer_class"] = json!("model");
        grant["reasoning_limits_v2"]["methods"] = json!(["learned"]);
        grant["reasoning_limits_v2"]["resources"]["max_output_tokens"] = json!(64);
        f.write("assessor-grant.json", grant);
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        until(&mut host, &mut runtime, |r| {
            r.report(0).unwrap().stopped_plans == 1
        });
        assert_eq!(
            runtime.plans_after(0, 1).unwrap()[0].state,
            PlanState::Rejected
        );
        assert_eq!(acts(&f), 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn plan_and_individual_goal_cancellation_reap_the_actual_goal_phase_invocation() {
    use arcs::reasoning::{ContextSelection, ReasoningError, ReasoningRequestV2};
    use arcs_host::HostError;
    for whole_plan in [true, false] {
        let f = fixture();
        std::fs::write(
            f.directory.join("goal-worker.py"),
            include_str!("support/planner_peer.py"),
        )
        .unwrap();
        f.write("goal-worker.json",json!({"adapter_id":"goal.planner","journal_name":"goal-worker","target_schema_id":"arcs.fan_candidate.v1","context_namespace":"goal_runtime","iterations":1,"required_capabilities":[{"adapter_id":"demo.fan","capability_id":"fan.set"}],"proposal":{"device_id":"fan-01","enabled":true},"gate":true}));
        let mut config = f.read("host.json");
        config["adapters"][2] = json!({"program":"/usr/bin/python3","args":[{"path":"goal-worker.py"},{"path":"goal-worker.json"}],"grant_file":"planner-grant.json","timeout_ms":15000});
        f.write("host.json", config);
        let mut host = f.host();
        let mut runtime = f.runtime(&host);
        until(&mut host, &mut runtime, |_| {
            f.directory.join("goal-worker.started").exists()
        });
        let goal = runtime.goals_after(0, 1).unwrap().remove(0);
        assert_eq!(goal.state, GoalState::Planning);
        if whole_plan {
            runtime
                .cancel_plan(&mut host, 1, "stop goal planner".into())
                .unwrap();
        } else {
            runtime
                .cancel_goal(&host, goal.sequence, "stop individual goal planner".into())
                .unwrap();
        }
        let route: arcs_host::task_runtime::ReasoningRoute = serde_json::from_value(
            f.read("runtime.json")["routes"][0]["plan"]["templates"][0]["route"].clone(),
        )
        .unwrap();
        let probe = ReasoningRequestV2 {
            request_id: "cancelled-session-reuse-probe".into(),
            capability: route.reasoner,
            method: route.method,
            objective: "check the same session can handle an independent request".into(),
            context: vec![ContextSelection {
                version_id: goal.input_version_id,
                payload_fields: route.payload_fields,
            }],
            target_schema_id: route.target_schema_id,
            allowed_capabilities: vec![],
            constraints: json!({"goal_runtime":{}}),
            budget: route.budget,
        };
        assert!(matches!(
            host.reason(probe.clone()),
            Err(HostError::AdapterBusy(_))
        ));
        std::fs::write(f.directory.join("goal-worker.release"), "go").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            runtime.tick(&mut host).unwrap();
            match host.reason(probe.clone()) {
                Err(HostError::AdapterBusy(_)) => {}
                Err(HostError::Reasoning(ReasoningError::ForbiddenCandidateCapability(_))) => break,
                other => panic!("unexpected independent probe result: {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "cancelled goal invocation kept its session"
            );
            std::thread::sleep(Duration::from_millis(3));
        }
        assert_eq!(
            f.db("goal-worker")
                .query_row("SELECT COUNT(*) FROM attempts", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(count(&host, "arcs.fan_candidate.v1"), 0);
        assert_eq!(acts(&f), 0);
        assert_eq!(
            runtime.goals_after(0, 1).unwrap()[0].state,
            GoalState::Cancelled
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn bundled_rules_workflow_and_cli_review_use_the_same_dynamic_goal_contract() {
    let f = fixture();
    f.write(
        "root-example-profile.json",
        json!({"adapter_id":"planner","method":"rules","scenario":"plan_fan"}),
    );
    let mut grant = f.read("root-grant.json");
    grant["producer_class"] = json!("adapter");
    grant["reasoning_limits_v2"]["methods"] = json!(["rules"]);
    f.write("root-grant.json", grant);
    let mut host_config = f.read("host.json");
    host_config["adapters"][4] = json!({"program":support::adapter_binary("arcs-example-reasoner"),"profile":"root-example-profile.json","args":[{"path":"root-planner.sqlite"}],"grant_file":"root-grant.json","timeout_ms":15000});
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["routes"][0]["method"] = json!("rules");
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let goal = awaiting(&mut host, &mut runtime);
    assert_eq!(acts(&f), 0);
    f.write("review.json",json!({"command":"review","sequence":goal.sequence,"candidate_version_id":goal.candidate,"decision":"approved","operator_id":"operator.simon","reason":"reviewed concrete simulated fan action"}));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arcs-host"))
        .current_dir(&f.directory)
        .args([
            "--config",
            "host.json",
            "--runtime",
            "runtime.json",
            "--goal-command",
            "review.json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["goals"][0]["state"], "executing");
    assert_eq!(acts(&f), 0);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(acts(&f), 1);
    assert_eq!(
        f.db("root-planner")
            .query_row("SELECT COUNT(*) FROM invocations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn a_generated_goal_waits_a_month_at_capacity_then_requires_fresh_observations_and_a_new_approval()
{
    fn advance(
        host: &mut Host,
        runtime: &mut TaskRuntime,
        now: u64,
        condition: impl Fn(&TaskRuntime) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            runtime.tick_at(host, now).unwrap();
            if condition(runtime) {
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
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(2);
    config["routes"][0]["plan"]["max_steps"] = json!(1);
    config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["between_rounds"] =
        json!({"kind":"after_delay","delay_ms":2_592_000_000u64});
    config["routes"][0]["plan"]["templates"][0]["route"]["goal"]["sources"].as_array_mut().unwrap().push(json!({"id":"observed-fan","capability":{"adapter_id":"demo.fan","capability_id":"fan.observe"},"schema_id":"arcs.state.fan.v1","payload_fields":["device_id","enabled","actuation_count"]}));
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":false,"minimum_actuations":2});
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let first = awaiting(&mut host, &mut runtime);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 2);
    review(&host, &mut runtime, &first, ApprovalDecision::Approved);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10).unwrap()[0].state == GoalState::WaitingTime
    });
    let schedule = runtime.scheduled_after(0, 10).unwrap().remove(0);
    let due = schedule.not_before_unix_ms.unwrap();
    let plan = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone();
    assert_eq!(acts(&f), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick_at(&mut host, due - 1).unwrap();
    assert_eq!(
        runtime.goals_after(0, 10).unwrap()[0].state,
        GoalState::WaitingTime
    );
    advance(&mut host, &mut runtime, due, |r| {
        r.goals_after(0, 10).unwrap()[0].state == GoalState::AwaitingApproval
    });
    let second = runtime.goals_after(0, 10).unwrap().remove(0);
    assert_eq!(second.round, 2);
    assert_ne!(first.candidate, second.candidate);
    assert!(second.approval.is_none());
    assert_eq!(second.before.len(), 2);
    assert_eq!(acts(&f), 1);
    assert!(
        runtime
            .review_goal(
                &host,
                second.sequence,
                first.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::Human,
                    id: "stale.review".into()
                },
                "old candidate".into()
            )
            .is_err()
    );
    review(&host, &mut runtime, &second, ApprovalDecision::Approved);
    advance(&mut host, &mut runtime, due, |r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(acts(&f), 2);
    assert_eq!(count(&host, "arcs.approval.v1"), 2);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].plan_version_id, plan);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

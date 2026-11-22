#[path = "support/goals.rs"]
mod goal_support;
#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs::reasoning::selection::{CONSTRAINT, SCHEMA};
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime, goal::GoalState, planning::PlanState},
};
use parallel_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn choice(route: &Value, adapter: &str) -> Value {
    json!({"capability":{"adapter_id":adapter,"capability_id":"solution.propose"},"method":route["method"],"budget":route["budget"]})
}

#[test]
fn goal_choices_require_a_receiver_grant_for_declared_binary_inputs() {
    let f = goal_support::Fixture::new();
    let mut config = f.read("runtime.json");
    select(&mut config["routes"][0], "goal.assessor");
    config["routes"][0]["goal"]["sources"][0]["binary"] = json!({"name":"frame","source":{"adapter_id":"demo.fan","capability_id":"fan.read"},"reference_pointer":"/frame"});
    f.write("runtime.json", config);
    let mut host = f.host();
    let error = match TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    ) {
        Ok(_) => panic!("missing binary grant accepted"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("binary input receiver"),
        "{error}"
    );
    assert!(!f.directory.join("runtime.sqlite").exists());
    host.shutdown().unwrap();
}
fn select(route: &mut Value, adapter: &str) {
    route["reasoner_selection"] = json!({"alternatives":[choice(route,adapter)]});
}
fn until(host: &mut Host, runtime: &mut TaskRuntime, condition: impl Fn(&TaskRuntime) -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = runtime.tick(host).unwrap();
        assert!(tick.failed.is_empty(), "{:?}", tick.failed);
        if condition(runtime) {
            return;
        }
        assert!(Instant::now() < end, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn simple(a: &str, b: &str) -> Fixture {
    let f = Fixture::new();
    f.peer("a", a, false);
    f.peer("b", b, false);
    f.route("main", "a");
    let mut config = f.read("runtime.json");
    select(&mut config["routes"][0], "b");
    f.write("runtime.json", config);
    f
}
fn records(host: &Host) -> Vec<arcs::core::Artifact> {
    plan_support::artifacts(host, SCHEMA)
}
fn describe(f: &Fixture, id: &str, purpose: &str) {
    let mut profile = f.read(&format!("{id}.json"));
    profile["description"] = json!({"purpose":purpose});
    f.write(&format!("{id}.json"), profile);
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    if !source.contains("c.get('description')") {
        std::fs::write(path,source.replace("  reply={'type':'manifest'","  if c.get('description'): caps[0]['description']=c['description']\n  reply={'type':'manifest'")).unwrap();
    }
}

#[test]
fn round_selection_requires_a_goal_route() {
    let f = simple("normal", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["reasoner_selection"]["reselect_each_round"] = json!(true);
    f.write("runtime.json", config);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let result = TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    );
    let Err(error) = result else {
        panic!("round selection accepted on an ordinary route");
    };
    assert!(error.to_string().contains("requires a goal route"));
    host.shutdown().unwrap();
}

#[test]
fn occupied_named_capacity_selects_an_alternative_and_replay_keeps_its_binding() {
    let f = simple("normal", "normal");
    f.peer("blocker", "gate", false);
    f.route("hold", "blocker");
    let mut config = f.read("runtime.json");
    let routes = config["routes"].as_array_mut().unwrap();
    routes.swap(0, 1);
    f.write("runtime.json", config);
    let mut host_config = f.read("host.json");
    host_config["resource_pools"] = json!({"gpu":1});
    for index in [1, 3] {
        host_config["adapters"][index]["reasoning_resources"] = json!({"gpu":1});
    }
    f.write("host.json", host_config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("blocker") && r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 0);
    assert_eq!(f.count("b", "attempts"), 1);
    let receipt = records(&host).remove(0);
    assert_eq!(receipt.payload["selected_index"], 1);
    assert_eq!(receipt.payload["observations"][0]["availability"], "busy");
    let summary = runtime.reasoner_selections(&host).unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].selected.capability.adapter_id.0, "b");
    let done = runtime.completed_after(0, 10).unwrap().remove(0);
    let request = host.store().get(&done.request_version_id).unwrap().unwrap();
    assert_eq!(
        request.payload["invocation"]["constraints"][CONSTRAINT],
        json!(receipt.version_id)
    );
    assert!(
        request
            .provenance
            .unwrap()
            .parents
            .contains(&receipt.version_id.0)
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("blocker");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(records(&host)[0].version_id, receipt.version_id);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 0);
    host.shutdown().unwrap();
}

#[test]
fn all_busy_choices_wait_without_a_record_and_select_when_one_is_actually_free() {
    let f = simple("gate", "gate");
    f.route("hold-a", "a");
    f.route("hold-b", "b");
    let mut config = f.read("runtime.json");
    let routes = config["routes"].as_array_mut().unwrap();
    routes.rotate_left(1);
    config["max_concurrent_reasoning"] = json!(4);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    assert!(records(&host).is_empty());
    let tick = runtime.tick(&mut host).unwrap();
    assert!(
        tick.waiting_resources
            .iter()
            .any(|w| w.resource == "reasoner-selection")
    );
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(
        runtime.reasoner_selections(&host).unwrap()[0]
            .selected
            .capability
            .adapter_id
            .0,
        "b"
    );
    assert_eq!(f.count("b", "attempts"), 2);
    f.release("a");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 3
    });
    host.shutdown().unwrap();
}

#[test]
fn a_core_selection_survives_a_failed_queue_checkpoint_and_does_not_switch_while_busy() {
    let f = simple("gate", "normal");
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(1);
    f.write("runtime.json", config.clone());
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_start BEFORE UPDATE ON runtime_jobs WHEN NEW.state='running' BEGIN SELECT RAISE(ABORT,'interrupted checkpoint'); END;").unwrap();
    assert!(runtime.tick(&mut host).is_err());
    let receipt = records(&host).remove(0);
    assert_eq!(receipt.payload["selected_index"], 0);
    assert_eq!(f.count("a", "attempts"), 0);
    db.execute_batch("DROP TRIGGER fail_start;").unwrap();
    drop(db);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    let mut other = config.clone();
    other["runtime_id"] = json!("occupy-after-restart");
    other["journal"] = json!(f.directory.join("other-runtime.sqlite"));
    other["max_concurrent_reasoning"] = json!(2);
    other["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("reasoner_selection");
    let mut other = TaskRuntime::open(&host, serde_json::from_value(other).unwrap()).unwrap();
    until(&mut host, &mut other, |_| f.started("a"));
    let mut runtime = TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    )
    .unwrap();
    runtime.tick(&mut host).unwrap();
    assert_eq!(f.count("b", "attempts"), 0);
    assert_eq!(records(&host)[0].version_id, receipt.version_id);
    f.release("a");
    until(&mut host, &mut other, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 0);
    host.shutdown().unwrap();
}

#[test]
fn private_input_fields_can_choose_a_reasoner_without_becoming_external_context() {
    let f = simple("normal", "normal");
    describe(&f, "a", "Calculate numeric results");
    describe(&f, "b", "Handle private_ticket_X9");
    let mut profile = f.read("input-profile.json");
    profile["events"][0]["payload"]["private_note"] = json!("private_ticket_X9");
    f.write("input-profile.json", profile);
    let mut config = f.read("runtime.json");
    config["routes"][0]["reasoner_selection"]["search_input_fields"] = json!(["private_note"]);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 0);
    assert_eq!(
        records(&host)[0].payload["policy"]["query_text"],
        "private_ticket_X9"
    );
    let request = plan_support::artifacts(&host, "arcs.reasoning_request.v2").remove(0);
    assert!(!request.payload.to_string().contains("private_ticket_X9"));
    assert!(
        request.payload["invocation"]["context"][0]["payload"]
            .get("private_note")
            .is_none()
    );
    assert!(
        !serde_json::to_string(&runtime.reasoner_selections(&host).unwrap())
            .unwrap()
            .contains("private_ticket_X9")
    );
    host.shutdown().unwrap();
}

#[test]
fn invalid_task_search_data_is_isolated_and_cannot_trigger_a_default_backend() {
    for case in ["no_match", "missing", "empty"] {
        let f = simple("normal", "normal");
        f.route("independent", "b");
        let mut config = f.read("runtime.json");
        if case == "missing" {
            config["routes"][0]["reasoner_selection"]["search_input_fields"] =
                json!(["missing_field"]);
        } else if case == "empty" {
            config["routes"][0]["reasoner_selection"]["search_input_fields"] =
                json!(["private_note"]);
            let mut input = f.read("input-profile.json");
            input["events"][0]["payload"]["private_note"] = json!("");
            f.write("input-profile.json", input);
        } else {
            config["routes"][0]["reasoner_selection"]["search_text"] = json!("no_such_purpose");
        }
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        let end = Instant::now() + Duration::from_secs(25);
        while runtime.report(0).unwrap().ready_for_review != 1 {
            runtime.tick(&mut host).unwrap();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
        assert_eq!(f.count("a", "attempts"), 0);
        assert_eq!(f.count("b", "attempts"), 1);
        assert!(runtime.reasoner_selections(&host).unwrap().is_empty());
        assert!(records(&host).is_empty());
        host.shutdown().unwrap();
    }
}

#[test]
fn a_goal_selected_planner_still_needs_manual_approval_and_confirmed_effect() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut host_config = f.read("host.json");
    let mut profile = f.read("planner-profile.json");
    profile["adapter_id"] = json!("goal.alternative");
    profile["description"] = json!({"purpose":"alternative planner"});
    f.write("alternative-profile.json", profile);
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("goal.alternative");
    f.write("alternative-grant.json", grant);
    let mut adapter = host_config["adapters"][2].clone();
    adapter["profile"] = json!("alternative-profile.json");
    adapter["grant_file"] = json!("alternative-grant.json");
    adapter["args"] = json!([{"path":f.directory.join("alternative.sqlite")}]);
    host_config["adapters"]
        .as_array_mut()
        .unwrap()
        .push(adapter);
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    select(&mut config["routes"][0], "goal.alternative");
    config["routes"][0]["reasoner_selection"]["search_text"] = json!("alternative");
    config["routes"][0]["reasoner_selection"]["reselect_each_round"] = json!(true);
    config["routes"][0]["goal"]["max_rounds"] = json!(1);
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 1)
            .unwrap()
            .first()
            .is_some_and(|g| g.state == GoalState::AwaitingApproval)
    });
    let goal = runtime.goals_after(0, 1).unwrap().remove(0);
    let candidate = host
        .store()
        .get(goal.candidate.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(candidate.created_by.id, "goal.alternative");
    let selections = runtime.reasoner_selections(&host).unwrap();
    assert_eq!(selections[0].round, Some(1));
    let receipt = records(&host).remove(0);
    assert_eq!(receipt.version_id, selections[0].selection_version_id);
    assert_eq!(receipt.payload["owner"]["task_id"], json!("1:round:1"));
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    runtime
        .review_goal(
            &host,
            goal.sequence,
            goal.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "operator.test".into(),
            },
            "reviewed simulated action".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().completed_goals == 1
    });
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(records(&host).len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn generated_steps_keep_their_selected_producer_through_the_join() {
    let f = plan_support::fixture("gate", "normal");
    let mut config = f.read("runtime.json");
    let mut hold = config["routes"][0]["plan"]["templates"][0]["route"].clone();
    hold["id"] = json!("hold-a");
    select(
        &mut config["routes"][0]["plan"]["templates"][0]["route"],
        "b",
    );
    config["routes"].as_array_mut().unwrap().insert(0, hold);
    config["max_pending_jobs"] = json!(5);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.plans_after(0, 1)
            .unwrap()
            .first()
            .is_some_and(|p| p.state == PlanState::ReadyForReview)
    });
    let selected = runtime.reasoner_selections(&host).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].selected.capability.adapter_id.0, "b");
    assert!(f.started("join"));
    assert_eq!(f.count("b", "attempts"), 2);
    f.release("a");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 5
    });
    host.shutdown().unwrap();
}

#[test]
fn an_alternative_root_planner_is_recognized_when_loading_accepted_steps() {
    let f = plan_support::fixture("normal", "normal");
    f.write(
        "planneralt.json",
        json!({"adapter_id":"planneralt","proposal":plan_support::proposal()}),
    );
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("planneralt");
    f.write("planneralt-grant.json", grant);
    let mut host_config = f.read("host.json");
    let mut adapter = host_config["adapters"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    adapter["args"] = json!([{"path":"planner.py"},{"path":"planneralt.json"}]);
    adapter["grant_file"] = json!("planneralt-grant.json");
    host_config["adapters"]
        .as_array_mut()
        .unwrap()
        .push(adapter);
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    select(&mut config["routes"][0], "planneralt");
    config["routes"][0]["reasoner_selection"]["search_text"] = json!("planneralt");
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.plans_after(0, 1)
            .unwrap()
            .first()
            .is_some_and(|p| p.state == PlanState::ReadyForReview)
    });
    let receipt = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(
        host.store()
            .get(receipt.proposal_version_id.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .created_by
            .id,
        "planneralt"
    );
    assert_eq!(f.count("planner", "attempts"), 0);
    assert_eq!(f.count("planneralt", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn plan_aggregate_limits_include_the_most_expensive_permitted_alternative() {
    for tokens in [false, true] {
        let f = plan_support::fixture("normal", "normal");
        let mut config = f.read("runtime.json");
        if tokens {
            for id in ["a", "b", "join"] {
                let mut grant = f.read(&format!("{id}-grant.json"));
                grant["reasoning_limits_v2"]["resources"]["max_output_tokens"] = json!(10);
                f.write(&format!("{id}-grant.json"), grant);
            }
            for template in config["routes"][0]["plan"]["templates"]
                .as_array_mut()
                .unwrap()
            {
                template["route"]["budget"]["max_output_tokens"] = json!(1);
            }
            config["routes"][0]["plan"]["max_total_output_tokens"] = json!(3);
        }
        let route = &mut config["routes"][0]["plan"]["templates"][0]["route"];
        select(route, "b");
        route["reasoner_selection"]["alternatives"][0]["budget"][if tokens {
            "max_output_tokens"
        } else {
            "max_runtime_ms"
        }] = json!(if tokens { 4 } else { 20000 });
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            !r.plans_after(0, 1).unwrap().is_empty()
        });
        assert_eq!(
            runtime.plans_after(0, 1).unwrap()[0].state,
            PlanState::Rejected
        );
        assert!(!f.started("a") && !f.started("b"));
        host.shutdown().unwrap();
    }
}

#[test]
fn invalid_options_and_external_selection_publishers_are_rejected() {
    for case in ["empty", "duplicate", "method", "budget", "role", "reserved"] {
        let f = simple("normal", "normal");
        if case == "reserved" {
            let mut grant = f.read("a-grant.json");
            grant["adapter_id"] = json!(arcs::reasoning::selection::PUBLISHER);
            f.write("a-grant.json", grant);
            assert!(Host::open(f.directory.join("host.json")).is_err());
            assert!(!f.directory.join("core.sqlite").exists());
            continue;
        }
        let mut config = f.read("runtime.json");
        let route = &mut config["routes"][0];
        match case {
            "empty" => route["reasoner_selection"]["alternatives"] = json!([]),
            "duplicate" => {
                route["reasoner_selection"]["alternatives"] = json!([choice(route, "a")])
            }
            "method" => route["reasoner_selection"]["alternatives"][0]["method"] = json!("solver"),
            "budget" => {
                route["reasoner_selection"]["alternatives"][0]["budget"]["max_runtime_ms"] =
                    json!(40000)
            }
            _ => {
                route["reasoner_selection"]["alternatives"][0]["capability"] =
                    json!({"adapter_id":"example.input","capability_id":"task.submit"})
            }
        }
        f.write("runtime.json", config);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err()
        );
        assert!(!f.directory.join("runtime.sqlite").exists());
        host.shutdown().unwrap();
    }
}

#[test]
fn a_frozen_cached_result_replays_while_its_adapter_is_busy_without_named_pools() {
    let f = simple("gate", "normal");
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE ON runtime_jobs WHEN NEW.state='ready_for_review' BEGIN SELECT RAISE(ABORT,'interrupted completion'); END;").unwrap();
    until(&mut host, &mut runtime, |_| f.started("a"));
    f.release("a");
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(
        plan_support::artifacts(&host, "arcs.reasoning_result.v2").len(),
        1
    );
    db.execute_batch("DROP TRIGGER fail_completion;").unwrap();
    std::fs::remove_file(f.directory.join("a.release")).unwrap();
    let mut other = f.read("runtime.json");
    other["runtime_id"] = json!("hold-cached-provider");
    other["journal"] = json!(f.directory.join("other-runtime.sqlite"));
    other["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("reasoner_selection");
    let mut other = TaskRuntime::open(&host, serde_json::from_value(other).unwrap()).unwrap();
    until(&mut host, &mut other, |_| f.count("a", "attempts") == 2);
    runtime.tick(&mut host).unwrap();
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 1);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 0);
    assert_eq!(records(&host).len(), 1);
    f.release("a");
    until(&mut host, &mut other, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    host.shutdown().unwrap();
}

#[test]
fn verified_attempts_break_a_tie_and_survive_restart_with_their_original_samples() {
    let f = simple("invalid", "normal");
    let mut config = f.read("runtime.json");
    let route = &mut config["routes"][0];
    route["reasoner_selection"]["history"] = json!({"lookback":4,"min_samples":1,
        "failure_weight":100,"latency_weight":0,"latency_reference_ms":1000});
    let budget = route["budget"].clone();
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let input = host.poll_inputs().unwrap().remove(0);
    let calibration = |id: &str, adapter: &str, budget: Value| {
        serde_json::from_value(json!({"request_id":id,"capability":{"adapter_id":adapter,"capability_id":"solution.propose"},"method":"learned",
            "objective":"Calibrate the technical response","context":[{"version_id":input.version_id,"payload_fields":["values","target"]}],
            "target_schema_id":"arcs.solution_candidate.nearest.v1","allowed_capabilities":[],"constraints":{},"budget":budget})).unwrap()
    };
    assert!(
        host.reason(calibration("bad", "a", budget.clone()))
            .is_err()
    );
    host.reason(calibration("good", "b", budget.clone()))
        .unwrap();
    let mut mismatched_budget = budget;
    mismatched_budget["max_runtime_ms"] = json!(9000);
    host.reason(calibration("different-budget", "b", mismatched_budget))
        .unwrap();
    let attempts = plan_support::artifacts(&host, arcs::reasoning::attempts::SCHEMA);
    assert_eq!(attempts.len(), 3);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    let saved = records(&host).remove(0);
    let observations = &saved.payload["observations"];
    assert_eq!(saved.payload["selected_index"], 1);
    assert_eq!(observations[0]["history"]["failures"], 1);
    assert_eq!(observations[1]["history"]["failures"], 0);
    assert_eq!(
        observations[0]["history"]["sample_versions"],
        json!([attempts[0].version_id])
    );
    assert_eq!(
        observations[1]["history"]["sample_versions"],
        json!([attempts[1].version_id])
    );
    let edges = arcs::store::ArtifactRelations::new(host.store())
        .outgoing(&saved.version_id)
        .unwrap();
    assert!(edges.iter().any(|edge| edge.to == attempts[0].version_id));
    assert!(edges.iter().any(|edge| edge.to == attempts[1].version_id));
    assert!(!edges.iter().any(|edge| edge.to == attempts[2].version_id));
    assert_eq!(
        plan_support::artifacts(&host, arcs::reasoning::attempts::SCHEMA).len(),
        4
    );
    host.shutdown().unwrap();
    drop((host, runtime));

    let (mut host, mut runtime) = f.open();
    runtime.tick(&mut host).unwrap();
    assert_eq!(records(&host), vec![saved.clone()]);
    let report = runtime.reasoner_selections(&host).unwrap().remove(0);
    assert_eq!(report.selected.capability.adapter_id.0, "b");
    assert_eq!(
        report.history.unwrap().sample_versions,
        vec![attempts[1].version_id.clone()]
    );
    assert_eq!(
        plan_support::artifacts(&host, arcs::reasoning::attempts::SCHEMA).len(),
        4
    );
    host.shutdown().unwrap();
}

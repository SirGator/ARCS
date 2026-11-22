#[path = "support/goals.rs"]
mod goals;
#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plans;
mod support;
use arcs_host::task_runtime::planning::PlanState;
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use parallel_support::Fixture;
use plans::{artifacts, assert_no_authority, until};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn fixture() -> Fixture {
    let f = plans::fixture("fail", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["budget"]["max_context_items"] = json!(4);
    config["routes"][0]["plan"]["replanning"] = json!({"max_revisions":1});
    config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(100000);
    for template in config["routes"][0]["plan"]["templates"]
        .as_array_mut()
        .unwrap()
    {
        template["route"]["constraints"] =
            json!({"operator_private":"PRIVATE_TEMPLATE_MUST_STAY_LOCAL"});
    }
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["proposal"] = json!({"steps":[{"id":"work","template":"a"}]});
    model["revisions"] = json!({"1":{"steps":[{"id":"work","template":"b"}]}});
    f.write("planner.json", model);
    let mut a = f.read("a.json");
    a["failure_message"] = json!("PRIVATE_ERROR_MUST_STAY_LOCAL");
    f.write("a.json", a);
    f
}
fn terminal(rt: &TaskRuntime) -> bool {
    rt.plans_after(0, 1).unwrap().first().is_some_and(|r| {
        [
            PlanState::ReadyForReview,
            PlanState::Blocked,
            PlanState::Rejected,
        ]
        .contains(&r.state)
    })
}
fn planner_inputs(f: &Fixture) -> Vec<Value> {
    let db = rusqlite::Connection::open(f.directory.join("planner.sqlite")).unwrap();
    let mut q = db
        .prepare("SELECT input FROM results ORDER BY rowid")
        .unwrap();
    q.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect()
}
fn release_revision(f: &Fixture, revision: u8) {
    std::fs::write(
        f.directory.join(format!("planner.r{revision}.release")),
        "go",
    )
    .unwrap();
}

#[test]
fn a_failed_step_yields_a_new_plan_without_replacing_the_original_task_or_old_artifact() {
    let f = fixture();
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists()
    });
    let before = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(before.state, PlanState::Replanning);
    assert_eq!(before.revisions.len(), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    let original = host
        .store()
        .get(before.plan_version_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(!f.started("b"));
    release_revision(&f, 1);
    until(&mut host, &mut runtime, terminal);
    let done = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(done.state, PlanState::ReadyForReview);
    assert_eq!(done.input_version_id, before.input_version_id);
    assert_eq!(done.revisions.len(), 2);
    assert_ne!(
        done.revisions[0].plan_version_id,
        done.revisions[1].plan_version_id
    );
    assert_eq!(done.steps.len(), 2);
    assert_eq!(done.steps[0].revision, 0);
    assert_eq!(done.steps[1].revision, 1);
    assert_ne!(done.steps[0].route_id, done.steps[1].route_id);
    assert_eq!(
        host.store().get(&original.version_id).unwrap().unwrap(),
        original
    );
    assert_eq!(artifacts(&host, "arcs.plan.v2").len(), 2);
    assert_eq!(artifacts(&host, "arcs.plan_replanning.v1").len(), 1);
    assert!(artifacts(&host, arcs::reasoning::plan_availability::SCHEMA).is_empty());
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(f.count("planner", "attempts"), 2);
    let inputs = planner_inputs(&f);
    assert_eq!(inputs.len(), 2);
    let raw = serde_json::to_string(&inputs).unwrap();
    assert!(!raw.contains("PRIVATE_TEMPLATE_MUST_STAY_LOCAL"));
    assert!(!raw.contains("PRIVATE_ERROR_MUST_STAY_LOCAL"));
    let task_id = |inv: &Value| {
        inv["context"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["schema_id"] == "arcs.task.nearest.v1")
            .unwrap()["version_id"]
            .clone()
    };
    assert_eq!(task_id(&inputs[0]), task_id(&inputs[1]));
    assert!(
        inputs[1]["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["schema_id"] != "arcs.plan.v2")
    );
    assert_eq!(inputs[1]["constraints"]["task_replanning"]["revision"], 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}
#[test]
fn max_revisions_stops_repeated_failure_and_releases_all_capacity() {
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["replanning"]["max_revisions"] = json!(2);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["revisions"] = json!({});
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::Blocked);
    assert_eq!(plan.revisions.len(), 3);
    assert_eq!(f.count("planner", "attempts"), 3);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(f.count("planner", "attempts"), 3);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}
#[test]
fn cumulative_runtime_is_charged_before_replanning_and_before_child_admission() {
    for (limit, expected_calls, expected_state) in [
        (25000, 1, PlanState::Blocked),
        (35000, 2, PlanState::Rejected),
    ] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(limit);
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, terminal);
        let plan = runtime.plans_after(0, 1).unwrap().remove(0);
        assert_eq!(plan.state, expected_state);
        assert_eq!(plan.revisions.len(), 1);
        assert_eq!(f.count("planner", "attempts"), expected_calls);
        assert!(!f.started("b"));
        assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}
#[test]
fn an_unsupported_replacement_and_a_no_solution_reply_never_create_new_steps() {
    for no_solution in [false, true] {
        let f = fixture();
        let mut model = f.read("planner.json");
        if no_solution {
            model["no_solution_revisions"] = json!([1]);
        } else {
            model["revisions"]["1"]["steps"][0]["template"] = json!("not-authorized");
        }
        f.write("planner.json", model);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, terminal);
        let plan = runtime.plans_after(0, 1).unwrap().remove(0);
        assert_eq!(plan.state, PlanState::Rejected);
        assert_eq!(plan.revisions.len(), 1);
        assert_eq!(plan.steps.len(), 1);
        assert!(!f.started("b"));
        assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}
#[test]
fn losing_a_replanning_response_preserves_the_exact_request_and_generation() {
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["include_template_availability"] = json!(true);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    model["select_available_template"] = json!(true);
    model.as_object_mut().unwrap().remove("revisions");
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists()
    });
    let context = artifacts(&host, "arcs.plan_replanning.v1")
        .remove(0)
        .version_id;
    let schema = arcs::reasoning::plan_availability::SCHEMA;
    let availability = artifacts(&host, schema);
    assert_eq!(availability.len(), 2);
    for (revision, first_reasoner) in [(0, "ready"), (1, "unavailable")] {
        let snapshot = &availability[revision];
        assert_eq!(snapshot.payload["revision"], revision);
        let templates = snapshot.payload["templates"].as_array().unwrap();
        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0]["id"], "a");
        assert_eq!(templates[0]["reasoners"][0]["availability"], first_reasoner);
        assert_eq!(templates[1]["reasoners"][0]["availability"], "ready");
    }
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    release_revision(&f, 1);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("planner", "attempts"), 3);
    assert_eq!(f.count("planner", "results"), 2);
    assert_eq!(artifacts(&host, "arcs.plan.v2").len(), 2);
    assert_eq!(
        artifacts(&host, "arcs.plan_replanning.v1")[0].version_id,
        context
    );
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(artifacts(&host, schema), availability);
    let requests = artifacts(&host, "arcs.reasoning_request.v2");
    for (index, input) in planner_inputs(&f).iter().enumerate() {
        let snapshot = &availability[index];
        let context = input["context"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["schema_id"] == schema)
            .unwrap();
        assert_eq!(context["version_id"], snapshot.version_id.0);
        assert_eq!(
            context["payload"],
            json!({"revision":index,"templates":snapshot.payload["templates"]})
        );
        let request = requests
            .iter()
            .find(|artifact| artifact.payload["invocation"]["request_id"] == input["request_id"])
            .unwrap();
        assert!(
            request
                .provenance
                .as_ref()
                .unwrap()
                .parents
                .contains(&snapshot.version_id.0)
        );
    }
    assert_no_authority(&host);
    host.shutdown().unwrap();
}
#[test]
fn template_availability_needs_an_extra_slot_alongside_replanning_and_dependencies() {
    for with_dependency in [false, true] {
        let f = fixture();
        if with_dependency {
            f.peer("precheck", "normal", false);
            f.route("precheck", "precheck");
        }
        let mut config = f.read("runtime.json");
        let owner = &mut config["routes"][0];
        owner["plan"]["include_template_availability"] = json!(true);
        owner["budget"]["max_context_items"] = json!(if with_dependency { 4 } else { 3 });
        if with_dependency {
            owner["dependencies"] = json!([{"route_id":"precheck","payload_fields":["value"]}]);
            config["max_pending_jobs"] = json!(5);
        }
        f.write("runtime.json", config);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
        let opened = TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
        );
        let Err(error) = opened else {
            panic!("planner with no room for availability was opened");
        };
        let required = if with_dependency { 5 } else { 4 };
        assert!(
            error
                .to_string()
                .contains(&format!("at least {required} planner context slots")),
            "{error}"
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn reasoning_request_limit_survives_checkpoint_rollback_replanning_and_restart() {
    use arcs_host::task_runtime::queue::{TaskQueueQuery, TaskQueueScope};

    let f = fixture();
    f.peer("side", "normal", false);
    f.route("independent", "side");
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_reasoning_requests"] = json!(3);
    config["max_pending_jobs"] = json!(5);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER lose_reasoning_binding BEFORE INSERT ON runtime_reasoning_requests
         WHEN NEW.sequence=1 BEGIN SELECT RAISE(ABORT,'simulated request binding rollback'); END;",
    )
    .unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host));
    assert_eq!(f.count("planner", "attempts"), 0);
    db.execute_batch("DROP TRIGGER lose_reasoning_binding")
        .unwrap();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists() && f.started("side")
    });
    let independent_queue = |runtime: &TaskRuntime| {
        runtime
            .task_queue(TaskQueueQuery {
                scope: TaskQueueScope::All,
                after_sequence: 0,
                through_sequence: None,
                limit: 10,
            })
            .unwrap()
    };
    let before = independent_queue(&runtime).tasks[0]
        .reasoning_budget
        .unwrap();
    assert_eq!((before.task_sequence, before.used, before.limit), (1, 3, 3));
    host.shutdown().unwrap();
    drop((host, runtime, db));

    release_revision(&f, 1);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |runtime| {
        terminal(runtime) && runtime.report(0).unwrap().ready_for_review == 1
    });
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::Blocked);
    assert_eq!(plan.revisions.len(), 2);
    let denied = plan.steps.iter().find(|step| step.revision == 1).unwrap();
    assert_eq!(denied.state, "budget_exhausted");
    assert_eq!(f.count("planner", "attempts"), 3);
    assert_eq!(f.count("planner", "results"), 2);
    assert_eq!(f.count("b", "attempts"), 0);
    assert_eq!(f.count("side", "attempts"), 1);
    let tasks = independent_queue(&runtime).tasks;
    for task in tasks
        .iter()
        .filter(|task| task.sequence == 1 || task.parent_plan_sequence == Some(1))
    {
        let budget = task.reasoning_budget.unwrap();
        assert_eq!((budget.task_sequence, budget.used, budget.limit), (1, 3, 3));
    }
    let side = tasks
        .iter()
        .find(|task| task.route_id == "independent")
        .unwrap();
    assert_eq!(side.state, "ready_for_review");
    assert!(side.reasoning_budget.is_none());
    assert_eq!(runtime.report(0).unwrap().blocked_goals, 0);
    host.shutdown().unwrap();
}

#[test]
fn a_published_replacement_survives_a_rolled_back_queue_commit_without_another_model_call() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_revision BEFORE INSERT ON runtime_plan_steps WHEN NEW.node_id LIKE 'revision:1:%' BEGIN SELECT RAISE(ABORT,'lost revision admission'); END;").unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(artifacts(&host, "arcs.plan.v2").len(), 2);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].revisions.len(), 1);
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].steps[0].state,
        "failed"
    );
    assert!(!f.started("b"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db.execute_batch("DROP TRIGGER lose_revision").unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(artifacts(&host, "arcs.plan.v2").len(), 2);
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}
#[test]
fn replanning_configuration_requires_bounded_revisions_and_valid_context() {
    for case in ["zero", "many", "context", "constraint"] {
        let f = fixture();
        let mut config = f.read("runtime.json");
        match case {
            "zero" => config["routes"][0]["plan"]["replanning"]["max_revisions"] = json!(0),
            "many" => config["routes"][0]["plan"]["replanning"]["max_revisions"] = json!(8),
            "context" => config["routes"][0]["budget"]["max_context_items"] = json!(2),
            "constraint" => {
                config["routes"][0]["constraints"] = json!({"task_replanning":{"revision":1}})
            }
            _ => unreachable!(),
        }
        f.write("runtime.json", config);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err(),
            "{case}"
        );
        assert!(!f.started("planner"));
        host.shutdown().unwrap();
    }
}

#[test]
fn completed_step_exports_are_available_without_private_template_fields_or_unselected_data() {
    use arcs_host::task_runtime::task_output::{TaskOutputCommand, TaskOutputResponse};
    let f = fixture();
    let mut model = f.read("planner.json");
    model["proposal"] = json!({"steps":[{"id":"done","template":"b"},{"id":"fails","template":"a","dependencies":[{"step_id":"done","payload_fields":["value"]}]}]});
    f.write("planner.json", model);
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["budget"]["max_context_items"] = json!(2);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::ReadyForReview);
    assert_eq!(plan.steps[0].state, "ready_for_review");
    let current = plan.steps.iter().find(|step| step.revision == 1).unwrap();
    let output = runtime
        .control_task_output(
            &mut host,
            TaskOutputCommand::Options {
                task_sequence: current.sequence,
            },
        )
        .unwrap();
    assert!(
        matches!(output, TaskOutputResponse::Options(options) if options.result_versions.len() == 1)
    );
    assert!(
        runtime
            .control_task_output(
                &mut host,
                TaskOutputCommand::Options {
                    task_sequence: plan.steps[0].sequence,
                }
            )
            .is_err()
    );
    let inputs = planner_inputs(&f);
    let second = &inputs[1];
    let output = second["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["schema_id"] == "arcs.solution_candidate.nearest.v1")
        .unwrap();
    assert_eq!(
        output["payload"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["value"]
    );
    let metadata = second["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["schema_id"] == "arcs.plan_replanning.v1")
        .unwrap();
    assert_eq!(metadata["payload"]["steps"][0]["resolution"], "ready");
    assert_eq!(
        metadata["payload"]["steps"][0]["output_version_ids"][0],
        output["version_id"]
    );
    assert_eq!(metadata["payload"]["steps"][1]["resolution"], "blocked");
    let raw = serde_json::to_string(second).unwrap();
    assert!(!raw.contains("PRIVATE_TEMPLATE_MUST_STAY_LOCAL"));
    assert!(!raw.contains("PRIVATE_ERROR_MUST_STAY_LOCAL"));
    assert_no_authority(&host);
    host.shutdown().unwrap();
}
fn export_reuse_fixture() -> Fixture {
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["budget"]["max_context_items"] = json!(2);
    config["routes"][0]["plan"]["replanning"]["reuse_completed_exports"] = json!(true);
    config["routes"][0]["target_schema_id"] = json!("arcs.plan_candidate.v2");
    config["routes"][0]["max_reasoning_requests"] = json!(6);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["target_schema_id"] = json!("arcs.plan_candidate.v2");
    model["proposal"] = json!({"steps":[
        {"id":"done","template":"b"},
        {"id":"fails","template":"a","dependencies":[{"step_id":"done","payload_fields":["value"]}]}
    ]});
    model["revisions"]["1"] = json!({"steps":[
        {"id":"reuse","template":"join","dependencies":[
            {"step_id":"done","previous_revision":true,"payload_fields":["value"]}
        ]},
        {"id":"total","template":"join","dependencies":[
            {"step_id":"done","previous_revision":true,"payload_fields":["value"]},
            {"step_id":"reuse","payload_fields":["value"]}
        ]}
    ]});
    f.write("planner.json", model);
    f
}

#[test]
fn previous_revision_exports_feed_pinned_and_live_dependencies_after_admission_rollback() {
    let f = export_reuse_fixture();
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER lose_reused_exports BEFORE INSERT ON runtime_job_joins
         WHEN NEW.bindings_json != '[]' AND EXISTS(
            SELECT 1 FROM runtime_jobs WHERE sequence=NEW.sequence AND route_id='p1.r1.total'
         )
         BEGIN SELECT RAISE(ABORT,'simulated predecessor export checkpoint rollback'); END;",
    )
    .unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host));
    let published = artifacts(&host, "arcs.plan.v3");
    assert_eq!(published.len(), 1);
    assert_eq!(artifacts(&host, "arcs.plan.v2").len(), 1);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].revisions.len(), 1);
    assert_eq!(f.count("join", "attempts"), 0);
    db.execute_batch("DROP TRIGGER lose_reused_exports")
        .unwrap();
    host.shutdown().unwrap();
    drop((host, runtime, db));

    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::ReadyForReview);
    assert_eq!(plan.plan_version_id, Some(published[0].version_id.clone()));
    assert_eq!(plan.revisions.len(), 2);
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("join", "attempts"), 2);
    let original = plan
        .steps
        .iter()
        .find(|step| step.revision == 0 && step.id == "done")
        .unwrap();
    let original_result = runtime
        .completed_after(original.sequence - 1, 1)
        .unwrap()
        .remove(0);
    let expected = &original_result.candidate_version_ids[0];
    let reuse = plan.steps.iter().find(|step| step.id == "reuse").unwrap();
    let total = plan.steps.iter().find(|step| step.id == "total").unwrap();
    for sequence in [reuse.sequence, total.sequence] {
        let join = runtime
            .dependencies_after(sequence - 1, 1)
            .unwrap()
            .remove(0);
        assert_eq!(join.sequence, sequence);
        assert_eq!(join.bindings[0].sequence, original.sequence);
        assert_eq!(&join.bindings[0].context[0].version_id, expected);
        assert_eq!(join.bindings[0].context[0].payload_fields, ["value"]);
        assert_eq!(
            join.bindings.len(),
            if sequence == total.sequence { 2 } else { 1 }
        );
    }
    let total_result = runtime
        .completed_after(total.sequence - 1, 1)
        .unwrap()
        .remove(0);
    let result = host
        .store()
        .get(&total_result.candidate_version_ids[0])
        .unwrap()
        .unwrap();
    let previous = host.store().get(expected).unwrap().unwrap();
    assert_eq!(
        result.payload["value"].as_f64(),
        previous.payload["value"].as_f64().map(|v| v * 2.0)
    );
    assert!(
        arcs::store::ArtifactRelations::new(host.store())
            .outgoing(&published[0].version_id)
            .unwrap()
            .iter()
            .any(
                |edge| &edge.to == expected && edge.kind == arcs::store::relation_kinds::based_on()
            )
    );
    let input = &planner_inputs(&f)[1];
    assert_eq!(
        input["constraints"]["task_replanning"]["previous_step_dependency"]["previous_revision"],
        true
    );
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn previous_revision_exports_can_feed_a_later_revision_from_a_v3_plan() {
    let f = export_reuse_fixture();
    f.peer("c", "fail", false);
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_reasoning_requests"] = json!(8);
    config["routes"][0]["plan"]["replanning"]["max_revisions"] = json!(2);
    let templates = config["routes"][0]["plan"]["templates"]
        .as_array_mut()
        .unwrap();
    let mut failed = templates[0].clone();
    failed["route"]["id"] = json!("c");
    failed["route"]["reasoner"]["adapter_id"] = json!("c");
    templates.push(failed);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["revisions"]["1"]["steps"][1] = json!({
        "id":"fails_again","template":"c","dependencies":[{"step_id":"reuse","payload_fields":["value"]}]
    });
    model["revisions"]["2"] = json!({"steps":[
        {"id":"final","template":"join","dependencies":[
            {"step_id":"reuse","previous_revision":true,"payload_fields":["value"]}
        ]}
    ]});
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::ReadyForReview);
    assert_eq!(plan.revisions.len(), 3);
    assert_eq!(artifacts(&host, "arcs.plan.v3").len(), 2);
    let predecessor = plan
        .steps
        .iter()
        .find(|step| step.revision == 1 && step.id == "reuse")
        .unwrap();
    let current = plan.steps.iter().find(|step| step.revision == 2).unwrap();
    let dependency = runtime
        .dependencies_after(current.sequence - 1, 1)
        .unwrap()
        .remove(0);
    assert_eq!(dependency.sequence, current.sequence);
    assert_eq!(dependency.bindings[0].sequence, predecessor.sequence);
    assert_eq!(f.count("b", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("c", "attempts"), 1);
    assert_eq!(f.count("join", "attempts"), 2);
    assert_eq!(f.count("planner", "attempts"), 3);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn previous_revision_dependencies_require_an_enabled_completed_visible_export() {
    for (case, rejection) in [
        ("disabled", "not enabled"),
        ("initial", "require a replacement plan"),
        ("failed", "completed predecessor step"),
        ("private", "exceeds the predecessor exports"),
        ("omitted", "omitted from the replanning context"),
        ("capacity", "context budget must reserve room"),
    ] {
        let f = export_reuse_fixture();
        let mut model = f.read("planner.json");
        let mut config = f.read("runtime.json");
        match case {
            "disabled" => {
                config["routes"][0]["plan"]["replanning"]["reuse_completed_exports"] = json!(false);
            }
            "initial" => {
                model["proposal"] = model["revisions"]["1"].clone();
            }
            "failed" => {
                model["revisions"]["1"]["steps"][0]["dependencies"][0]["step_id"] = json!("fails");
            }
            "private" => {
                model["revisions"]["1"]["steps"][0]["dependencies"][0]["payload_fields"] =
                    json!(["distance"]);
            }
            "omitted" => config["routes"][0]["budget"]["max_context_items"] = json!(3),
            "capacity" => {
                config["routes"][0]["plan"]["templates"][2]["route"]["budget"]["max_context_items"] =
                    json!(2);
            }
            _ => unreachable!(),
        }
        f.write("planner.json", model);
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, terminal);
        let plan = runtime.plans_after(0, 1).unwrap().remove(0);
        assert_eq!(plan.state, PlanState::Rejected, "{case}: {plan:?}");
        assert!(
            plan.detail
                .as_deref()
                .unwrap_or_default()
                .contains(rejection),
            "{case}: {plan:?}"
        );
        assert_eq!(plan.revisions.len(), usize::from(case != "initial"));
        assert_eq!(f.count("join", "attempts"), 0);
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}

#[test]
fn cumulative_token_budget_cannot_be_reset_by_a_new_plan() {
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["routes"][0]["budget"]["max_output_tokens"] = json!(100);
    config["routes"][0]["plan"]["max_total_output_tokens"] = json!(350);
    for t in config["routes"][0]["plan"]["templates"]
        .as_array_mut()
        .unwrap()
    {
        t["route"]["budget"]["max_output_tokens"] = json!(100);
    }
    f.write("runtime.json", config);
    for id in ["a", "b", "join", "planner"] {
        let name = format!("{id}-grant.json");
        let mut grant = f.read(&name);
        grant["reasoning_limits_v2"]["resources"]["max_output_tokens"] = json!(100);
        f.write(&name, grant);
    }
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::Rejected);
    assert_eq!(plan.revisions.len(), 1);
    assert_eq!(f.count("planner", "attempts"), 2);
    assert!(!f.started("b"));
    host.shutdown().unwrap();
}
#[test]
fn reserved_headroom_allows_replanning_at_capacity_before_admitting_the_next_input() {
    let f = fixture();
    let mut input = f.read("input-profile.json");
    input["events"].as_array_mut().unwrap().push(json!({"id":"second","subject":"second-project","reference":"second-task","payload":{"values":[2,8],"target":9}}));
    f.write("input-profile.json", input);
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists()
    });
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    assert_eq!(runtime.plans_after(0, 10).unwrap().len(), 1);
    release_revision(&f, 1);
    until(&mut host, &mut runtime, |r| {
        r.plans_after(0, 10).unwrap().len() == 2
            && r.plans_after(0, 10)
                .unwrap()
                .iter()
                .all(|p| p.state == PlanState::ReadyForReview)
    });
    let plans = runtime.plans_after(0, 10).unwrap();
    assert_ne!(plans[0].input_version_id, plans[1].input_version_id);
    assert!(plans.iter().all(|p| p.revisions.len() == 2));
    assert_eq!(f.count("planner", "attempts"), 4);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}
#[test]
fn a_bad_replacement_payload_stops_replanning_without_inventing_an_accepted_revision() {
    let f = fixture();
    let mut model = f.read("planner.json");
    model["revisions"]["1"]["steps"][0]["program"] = json!("/bin/sh");
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::Blocked);
    assert_eq!(plan.revisions.len(), 1);
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 2);
    assert_eq!(plan.steps[0].state, "failed");
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert!(!f.started("b"));
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}
#[test]
fn old_unfinished_branches_are_cancelled_and_late_answers_do_not_satisfy_new_dependencies() {
    let f = fixture();
    let mut b = f.read("b.json");
    b["mode"] = json!("gate");
    f.write("b.json", b);
    let mut model = f.read("planner.json");
    model["proposal"] =
        json!({"steps":[{"id":"fails","template":"a"},{"id":"old","template":"b"}]});
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists()
    });
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert!(
        plan.steps
            .iter()
            .any(|s| s.id == "fails" && s.state == "failed")
    );
    assert!(
        plan.steps
            .iter()
            .any(|s| s.id == "old" && s.state == "cancelled")
    );
    assert!(f.started("b"));
    f.release("b");
    release_revision(&f, 1);
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("b", "results"), 2);
    let candidates = artifacts(&host, "arcs.solution_candidate.nearest.v1");
    assert_eq!(candidates.len(), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

fn goal_fixture(actions: usize) -> goals::Fixture {
    let f = goals::plan_fixture();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":false,"minimum_actuations":2});
    f.write("input-profile.json", input);
    let mut config = f.read("runtime.json");
    config
        .as_object_mut()
        .unwrap()
        .remove("max_concurrent_reasoning");
    let plan = &mut config["routes"][0]["plan"];
    plan["replanning"] = json!({"max_revisions":1});
    plan["max_total_runtime_ms"] = json!(200000);
    plan["max_total_actions"] = json!(actions);
    let mut first = plan["templates"][0].clone();
    first["route"]["id"] = json!("first");
    first["route"]["goal"]["max_rounds"] = json!(1);
    first["route"]["goal"]["initial_assessment"] = json!(true);
    first["route"]["goal"]["exclusive_resources"] = json!(["replanning.simulated-fan"]);
    let mut second = first.clone();
    second["route"]["id"] = json!("second");
    second["route"]["goal"]["max_rounds"] = json!(2);
    plan["templates"] = json!([first, second]);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["proposal"] = json!({"steps":[{"id":"device","template":"first"}]});
    model["revisions"] = json!({"1":{"steps":[{"id":"device","template":"second"}]}});
    f.write("planner.json", model);
    f
}
fn effect_count(f: &goals::Fixture) -> i64 {
    f.db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |r| r.get(0))
        .unwrap()
}
fn review(host: &Host, runtime: &mut TaskRuntime, task: &arcs_host::task_runtime::goal::GoalTask) {
    runtime
        .review_goal(
            host,
            task.sequence,
            task.candidate.as_ref().unwrap(),
            arcs::approval::ApprovalDecision::Approved,
            arcs::core::Actor {
                actor_type: arcs::core::ActorType::Human,
                id: "replanning.simulator-reviewer".into(),
            },
            "Explicit qualification of the local simulator only".into(),
        )
        .unwrap();
}
#[test]
fn replacement_goals_use_fresh_sources_and_require_their_own_manual_review() {
    use arcs_host::task_runtime::goal::GoalState;
    let f = goal_fixture(3);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().awaiting_approval == 1
    });
    let first = runtime.goals_after(0, 10).unwrap().remove(0);
    assert_eq!(effect_count(&f), 0);
    review(&host, &mut runtime, &first);
    until(&mut host, &mut runtime, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .iter()
            .any(|g| g.sequence != first.sequence && g.state == GoalState::AwaitingApproval)
    });
    let second = runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.sequence != first.sequence)
        .unwrap();
    assert_eq!(effect_count(&f), 1);
    assert_ne!(first.candidate, second.candidate);
    assert!(second.before.iter().all(|id| !first.before.contains(id)));
    assert!(
        runtime
            .review_goal(
                &host,
                second.sequence,
                first.candidate.as_ref().unwrap(),
                arcs::approval::ApprovalDecision::Approved,
                arcs::core::Actor {
                    actor_type: arcs::core::ActorType::Human,
                    id: "replanning.simulator-reviewer".into()
                },
                "Reject an obsolete plan approval".into()
            )
            .is_err()
    );
    review(&host, &mut runtime, &second);
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(effect_count(&f), 2);
    let done = runtime.goals_after(0, 10).unwrap();
    assert_eq!(done[0].state, GoalState::BudgetExhausted);
    assert_eq!(done[1].state, GoalState::Completed);
    let outcome = done[1].outcome.clone();
    let plan = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(effect_count(&f), 2);
    assert_eq!(runtime.goals_after(0, 10).unwrap()[1].outcome, outcome);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].plan_version_id, plan);
    host.shutdown().unwrap();
}
#[test]
fn allocated_action_budget_is_not_refunded_when_a_goal_fails_to_finish() {
    let f = goal_fixture(2);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().awaiting_approval == 1
    });
    let first = runtime.goals_after(0, 10).unwrap().remove(0);
    review(&host, &mut runtime, &first);
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Rejected
    );
    assert_eq!(effect_count(&f), 1);
    assert_eq!(runtime.goals_after(0, 10).unwrap().len(), 1);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].revisions.len(), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}
#[test]
fn paused_replanning_collects_a_proposal_without_admitting_new_steps_until_resume() {
    use arcs_host::task_runtime::suspension::{TaskChange, TaskControlCommand};
    let f = fixture();
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.directory.join("planner.r1.started").exists()
    });
    let root = runtime.plans_after(0, 1).unwrap()[0].sequence;
    let command = |revision, id: &str, change| TaskControlCommand {
        sequence: root,
        expected_revision: revision,
        command_id: id.into(),
        operator: arcs::core::Actor {
            actor_type: arcs::core::ActorType::Human,
            id: "replan-control".into(),
        },
        reason: "qualification".into(),
        change,
    };
    runtime
        .control_task(&mut host, command(0, "pause", TaskChange::Pause))
        .unwrap();
    release_revision(&f, 1);
    until(&mut host, &mut runtime, |_| {
        let db = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM artifact_versions WHERE schema_id='arcs.plan_candidate.v1'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            == 2
    });
    assert!(!f.started("b"));
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].revisions.len(), 1);
    runtime
        .control_task(
            &mut host,
            command(
                1,
                "resume",
                TaskChange::Resume {
                    recover_dispatched: false,
                },
            ),
        )
        .unwrap();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("planner", "attempts"), 2);
    host.shutdown().unwrap();
}
#[test]
fn original_task_deadline_cancels_a_pending_replan_and_is_not_reset_by_its_revision() {
    let f = fixture();
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(10000);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["gate_revision"] = json!(1);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    let deadline = Instant::now() + Duration::from_secs(25);
    while !f.directory.join("planner.r1.started").exists() {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(
        runtime.deadlines_after(0, 10).unwrap()[0].expires_at_unix_ms,
        1_800_000_010_000
    );
    runtime.tick_at(&mut host, 1_800_000_010_001).unwrap();
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Cancelled
    );
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    release_revision(&f, 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..3 {
        runtime.tick_at(&mut host, 1_800_000_010_002).unwrap();
    }
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].revisions.len(), 1);
    assert!(!f.started("b"));
    assert_eq!(f.count("planner", "attempts"), 2);
    host.shutdown().unwrap();
}
#[test]
fn private_plan_payloads_cannot_be_smuggled_into_the_replanning_context() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.plans_after(0, 1)
            .unwrap()
            .first()
            .is_some_and(|p| p.state == PlanState::Replanning)
    });
    assert!(!f.directory.join("planner.r1.started").exists());
    let original = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone()
        .unwrap();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT context_json FROM runtime_plan_replanning",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut context: Value = serde_json::from_str(&raw).unwrap();
    context["context"] = json!([{"version_id":original,"payload_fields":["steps"]}]);
    db.execute(
        "UPDATE runtime_plan_replanning SET context_json=?1",
        [serde_json::to_string(&context).unwrap()],
    )
    .unwrap();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Blocked
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert!(!f.started("b"));
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn a_fallback_planner_is_bound_to_its_own_revision_while_prior_origins_remain_valid() {
    let f = fixture();
    let mut model = f.read("planner.json");
    model["fail_revisions"] = json!([1]);
    f.write("planner.json", model.clone());
    model["adapter_id"] = json!("planner2");
    model["fail_revisions"] = json!([]);
    f.write("planner2.json", model);
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("planner2");
    f.write("planner2-grant.json", grant);
    let mut hostconfig = f.read("host.json");
    hostconfig["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"planner.py"},{"path":"planner2.json"}],"grant_file":"planner2-grant.json","timeout_ms":15000}));
    f.write("host.json", hostconfig);
    let mut config = f.read("runtime.json");
    let budget = config["routes"][0]["budget"].clone();
    config["routes"][0]["reasoning_recovery"] = json!({"delay_ms":0,"alternatives":[{"capability":{"adapter_id":"planner2","capability_id":"solution.propose"},"method":"learned","budget":budget}]});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::ReadyForReview);
    assert_eq!(plan.revisions.len(), 2);
    let old = host
        .store()
        .get(&plan.revisions[0].proposal_version_id)
        .unwrap()
        .unwrap();
    let new = host
        .store()
        .get(&plan.revisions[1].proposal_version_id)
        .unwrap()
        .unwrap();
    assert_eq!(old.created_by.id, "planner");
    assert_eq!(new.created_by.id, "planner2");
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(f.count("planner2", "attempts"), 1);
    let fallback = rusqlite::Connection::open(f.directory.join("planner2.sqlite")).unwrap();
    let raw: String = fallback
        .query_row("SELECT input FROM results", [], |r| r.get(0))
        .unwrap();
    assert!(!raw.contains("PRIVATE_PLANNER_ERROR_MUST_STAY_LOCAL"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("planner2", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_failed_intent_commit_keeps_the_previous_jobs_and_can_be_recovered_after_restart() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_intent BEFORE INSERT ON runtime_plan_replanning BEGIN SELECT RAISE(ABORT,'lost replanning checkpoint'); END;").unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let before = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(before.state, PlanState::Running);
    assert_eq!(before.steps[0].state, "failed");
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM runtime_plan_replanning", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(artifacts(&host, "arcs.plan_replanning.v1").len(), 0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db.execute_batch("DROP TRIGGER lose_intent").unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}
#[test]
fn a_failed_context_publication_does_not_turn_a_committed_intent_into_a_business_failure() {
    let f = fixture();
    let (mut host, mut runtime) = f.open();
    let db = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_context BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.plan_replanning.v1' BEGIN SELECT RAISE(ABORT,'lost context publication'); END;").unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::Replanning
    );
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].steps[0].state,
        "failed"
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert!(artifacts(&host, "arcs.plan_replanning.v1").is_empty());
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db.execute_batch("DROP TRIGGER lose_context").unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, terminal);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].state,
        PlanState::ReadyForReview
    );
    assert_eq!(f.count("planner", "attempts"), 2);
    host.shutdown().unwrap();
}

#[test]
fn goal_replanning_requires_explicit_exclusive_control_resource_identities() {
    let f = goal_fixture(3);
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][1]["route"]["goal"]["exclusive_resources"] = json!([]);
    f.write("runtime.json", config);
    let mut host = f.host();
    assert!(
        TaskRuntime::open(
            &host,
            RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
        )
        .is_err()
    );
    assert_eq!(effect_count(&f), 0);
    host.shutdown().unwrap();
}

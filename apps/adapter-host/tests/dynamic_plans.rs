#[path = "support/parallel.rs"]
mod parallel_support;
mod support;

use arcs_host::task_runtime::planning::PlanState;
use arcs_host::{
    Host,
    task_runtime::{RuntimeConfig, TaskRuntime},
};
use serde_json::json;
use std::time::{Duration, Instant};

#[path = "support/plans.rs"]
mod plan_support;
use plan_support::*;

#[test]
fn a_reasoner_proposes_a_plan_and_independent_steps_execute_before_the_join() {
    let f = fixture("gate", "gate");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| {
        f.started("a") && f.started("b")
    });
    assert!(!f.started("join"));
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    let receipt = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(receipt.state, PlanState::Running);
    assert_eq!(receipt.steps.len(), 3);
    let plan = host
        .store()
        .get(receipt.plan_version_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(plan.schema_id.0, "arcs.plan.v1");
    assert_eq!(plan.trust.level, arcs::core::TrustLevel::Low);
    assert_eq!(artifacts(&host, "arcs.plan_candidate.v1").len(), 1);
    assert_eq!(
        arcs::store::ArtifactRelations::new(host.store())
            .outgoing(&plan.version_id)
            .unwrap()
            .len(),
        3
    );
    f.release("b");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert!(!f.started("join"));
    f.release("a");
    until(&mut host, &mut runtime, |r| {
        r.plans_after(0, 1).unwrap()[0].state == PlanState::ReadyForReview
    });
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 4);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert_eq!(f.count("planner", "results"), 1);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn unapproved_templates_cycles_exports_and_aggregate_budgets_are_rejected_without_children() {
    for case in [
        "unknown",
        "cycle",
        "duplicate",
        "export",
        "size",
        "runtime",
        "tokens",
        "collision",
    ] {
        let f = fixture("normal", "normal");
        let mut model = f.read("planner.json");
        match case {
            "unknown" => model["proposal"]["steps"][0]["template"] = json!("not-authorized"),
            "cycle" => {
                model["proposal"]["steps"][0]["dependencies"] =
                    json!([{"step_id":"total","payload_fields":["value"]}])
            }
            "duplicate" => model["proposal"]["steps"][1]["id"] = json!("left"),
            "export" => {
                model["proposal"]["steps"][2]["dependencies"][0]["payload_fields"] =
                    json!(["distance"])
            }
            "size" => model["proposal"]["steps"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":"extra","template":"a"})),
            _ => {}
        }
        f.write("planner.json", model);
        let mut config = f.read("runtime.json");
        if case == "runtime" {
            config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(20000);
        }
        if case == "tokens" {
            config["routes"][0]["plan"]["max_total_output_tokens"] = json!(1);
        }
        if case == "collision" {
            config["routes"][0]["id"] = json!("p1.left");
        }
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            !r.plans_after(0, 1).unwrap().is_empty()
        });
        let receipt = runtime.plans_after(0, 1).unwrap().remove(0);
        assert_eq!(receipt.state, PlanState::Rejected, "{case}");
        assert!(receipt.steps.is_empty());
        assert!(receipt.plan_version_id.is_none());
        assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
        assert!(artifacts(&host, "arcs.plan.v1").is_empty());
        assert!(!f.started("a") && !f.started("b") && !f.started("join"));
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}

#[test]
fn schema_overrides_and_no_solution_never_create_an_executable_plan() {
    for schema_override in [false, true] {
        let f = fixture("normal", "normal");
        let mut model = f.read("planner.json");
        if schema_override {
            model["proposal"]["steps"][0]["program"] = json!("/bin/sh");
        } else {
            model["no_solution"] = json!(true);
        }
        f.write("planner.json", model);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, |r| {
            if schema_override {
                r.report(0).unwrap().failed_jobs == 1
            } else {
                !r.plans_after(0, 1).unwrap().is_empty()
            }
        });
        assert!(artifacts(&host, "arcs.plan.v1").is_empty());
        assert!(!f.started("a"));
        assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
        assert_no_authority(&host);
        host.shutdown().unwrap();
    }
}

#[test]
fn a_core_plan_published_before_a_failed_queue_commit_is_reused_without_duplicate_steps() {
    let f = fixture("normal", "normal");
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(1);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_expansion BEFORE INSERT ON runtime_plan_steps BEGIN SELECT RAISE(ABORT,'injected plan admission failure'); END;").unwrap();
    support::wait_for_interrupted_checkpoint(|| runtime.tick(&mut host));
    let plan = artifacts(&host, "arcs.plan.v1");
    assert_eq!(plan.len(), 1);
    assert!(runtime.plans_after(0, 10).unwrap().is_empty());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM runtime_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(!f.started("a"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    db.execute_batch("DROP TRIGGER reject_expansion;").unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert_eq!(artifacts(&host, "arcs.plan.v1").len(), 1);
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0]
            .plan_version_id
            .as_ref(),
        Some(&plan[0].version_id)
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("join", "attempts"), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

#[test]
fn restarting_an_accepted_plan_recovers_its_steps_without_replanning() {
    let f = fixture("gate", "normal");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| f.started("a"));
    let version = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("a");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].plan_version_id,
        version
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(f.count("a", "results"), 1);
    assert_eq!(f.count("a", "attempts"), 2);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn reservations_allow_two_full_expansions_at_capacity_and_keep_inputs_separate() {
    let f = fixture("normal", "normal");
    let mut input = f.read("input-profile.json");
    input["events"].as_array_mut().unwrap().push(json!({"id":"second","subject":"second-project","reference":"task:second","payload":{"values":[3,7],"target":6}}));
    f.write("input-profile.json", input);
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(8);
    f.write("runtime.json", config);
    let mut model = f.read("planner.json");
    model["gate"] = json!(true);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        f.started("planner") && r.report(0).unwrap().pending_jobs == 2
    });
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 2);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 8);
    assert!(runtime.plans_after(0, 10).unwrap().is_empty());
    f.release("planner");
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 8
    });
    let plans = runtime.plans_after(0, 10).unwrap();
    assert_eq!(plans.len(), 2);
    assert_ne!(plans[0].input_version_id, plans[1].input_version_id);
    for plan in &plans {
        for step in &plan.steps {
            let completion = runtime
                .completed_after(step.sequence - 1, 1)
                .unwrap()
                .remove(0);
            assert_eq!(completion.sequence, step.sequence);
            assert_eq!(completion.input_version_id, plan.input_version_id);
        }
    }
    assert_eq!(f.count("planner", "results"), 2);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

#[test]
fn a_failed_step_blocks_its_plan_and_dependents_while_independent_work_finishes() {
    let f = fixture("fail", "normal");
    f.route("independent", "b");
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(5);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        !r.plans_after(0, 1).unwrap().is_empty() && r.report(0).unwrap().pending_jobs == 0
    });
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    assert_eq!(plan.state, PlanState::Blocked);
    assert!(
        runtime
            .completed_after(0, 20)
            .unwrap()
            .iter()
            .any(|c| c.route_id == "independent")
    );
    assert_eq!(runtime.report(0).unwrap().blocked_dependencies, 1);
    assert!(!f.started("join"));
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn the_catalog_hides_template_constraints_but_the_authorized_step_keeps_them() {
    let f = fixture("normal", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["plan"]["templates"][0]["route"]["constraints"] =
        json!({"internal_policy":"synthetic-private-template-policy"});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    let planner = rusqlite::Connection::open(f.directory.join("planner.sqlite")).unwrap();
    let raw: String = planner
        .query_row("SELECT input FROM results", [], |r| r.get(0))
        .unwrap();
    assert!(!raw.contains("synthetic-private-template-policy"));
    let leaf = rusqlite::Connection::open(f.directory.join("a.sqlite")).unwrap();
    let raw: String = leaf
        .query_row("SELECT input FROM results", [], |r| r.get(0))
        .unwrap();
    assert!(raw.contains("synthetic-private-template-policy"));
    host.shutdown().unwrap();
}

#[test]
fn unsafe_templates_nested_plans_and_insufficient_expansion_capacity_are_rejected_early() {
    let f = fixture("normal", "normal");
    let original = f.read("runtime.json");
    let mut variants = Vec::new();
    let mut capacity = original.clone();
    capacity["max_pending_jobs"] = json!(3);
    variants.push(capacity);
    let mut nested = original.clone();
    nested["routes"][0]["plan"]["templates"][0]["route"]["plan"] =
        original["routes"][0]["plan"].clone();
    variants.push(nested);
    let mut caps = original.clone();
    caps["routes"][0]["plan"]["templates"][0]["route"]["allowed_capabilities"] =
        json!([{"adapter_id":"other","capability_id":"act"}]);
    variants.push(caps);
    let mut fields = original.clone();
    fields["routes"][0]["plan"]["templates"][0]["export_fields"] = json!([]);
    variants.push(fields);
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
fn a_lost_planner_response_reuses_its_proposal_and_capacity_reservation() {
    let f = fixture("normal", "normal");
    let mut model = f.read("planner.json");
    model["gate"] = json!(true);
    f.write("planner.json", model);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |_| f.started("planner"));
    assert_eq!(runtime.report(0).unwrap().capacity_used, 4);
    assert!(artifacts(&host, "arcs.plan.v1").is_empty());
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.release("planner");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, |r| {
        r.report(0).unwrap().ready_for_review == 4
    });
    assert_eq!(f.count("planner", "results"), 1);
    assert_eq!(f.count("planner", "attempts"), 2);
    assert_eq!(artifacts(&host, "arcs.plan.v1").len(), 1);
    assert_eq!(runtime.plans_after(0, 1).unwrap()[0].steps.len(), 3);
    host.shutdown().unwrap();
}

#[test]
fn planned_steps_keep_their_operator_defined_waits_over_a_month_and_a_restart() {
    let f = fixture("normal", "normal");
    let mut config = f.read("runtime.json");
    const NOW: u64 = 1_800_000_000_000;
    const MONTH: u64 = 2_592_000_000;
    config["routes"][0]["plan"]["templates"][0]["route"]["activation"] =
        json!({"kind":"after_delay","delay_ms":MONTH});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let until = Instant::now() + Duration::from_secs(25);
    while runtime.report(0).unwrap().ready_for_review != 1
        || runtime.report(0).unwrap().waiting_time != 1
    {
        runtime.tick_at(&mut host, NOW).unwrap();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(!f.started("a"));
    let version = runtime.plans_after(0, 1).unwrap()[0]
        .plan_version_id
        .clone();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, NOW + MONTH - 1).unwrap();
    assert!(!f.started("a"));
    let until = Instant::now() + Duration::from_secs(25);
    while runtime.report(0).unwrap().ready_for_review != 4 {
        runtime.tick_at(&mut host, NOW + MONTH).unwrap();
        assert!(Instant::now() < until, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(
        runtime.plans_after(0, 1).unwrap()[0].plan_version_id,
        version
    );
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    host.shutdown().unwrap();
}

#[test]
fn an_external_adapter_cannot_claim_the_core_plan_publishers_identity() {
    let f = fixture("normal", "normal");
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("arcs.host.plan");
    f.write("planner-grant.json", grant);
    let error = Host::open(f.directory.join("host.json"))
        .err()
        .expect("reserved publisher identity must be rejected");
    assert!(error.to_string().contains("reserved adapter ID"));
    assert!(!f.directory.join("core.sqlite").exists());
    assert!(!f.directory.join("planner.sqlite").exists());
}

use super::*;
use arcs_host::task_runtime::planning::PlanState;
use std::process::Command;

const EDIT_CANDIDATE: &str = "arcs.workspace_edit_candidate.v1";
const EDIT_RECEIPT: &str = "arcs.execution_result.workspace_edit.v1";
const CHECK_CANDIDATE: &str = "arcs.workspace_revision_check_candidate.v1";
const CHECK_RECEIPT: &str = "arcs.execution_result.workspace_revision_check.v1";
const TASK: &str = "arcs.task.workspace_repair.v1";
const REF: &str = "refs/arcs/repair";
const BROKEN: &str = "def add(left, right):\n    return left - right\n";
const FIXED: &str = "def add(left, right):\n    return left + right\n";

fn git(f: &Fixture, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .current_dir(f.path("project"))
        .args([
            "-c",
            "user.name=Test Maintainer",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn configure(f: &Fixture, export: &str) -> String {
    fs::write(f.path("project/calculator.py"), BROKEN).unwrap();
    git(f, &["init", "-q"]);
    git(f, &["add", "."]);
    git(f, &["commit", "-qm", "Starting project"]);
    let base = git(f, &["rev-parse", "HEAD"]).trim().to_owned();
    git(f, &["update-ref", REF, &base]);

    let mut input = f.read("input-profile.json");
    input["schema_id"] = json!(TASK);
    input["events"][0]["payload"] =
        json!({"check":"unit","path":"calculator.py","base_commit":base,"replacement":FIXED});
    f.write("input-profile.json", input);
    let mut reasoner = f.read("reasoner-profile.json");
    reasoner["scenario"] = json!("workspace_repair");
    f.write("reasoner-profile.json", reasoner);
    let mut checker = f.read("check-profile.json");
    checker["edit"] = json!({"reference":REF,"files":["calculator.py"]});
    checker["revision_checks"] = json!(["unit"]);
    f.write("check-profile.json", checker);
    let mut grant = f.read("check-grant.json");
    grant["enabled_capabilities"] = json!(["edit.apply", "check.revision"]);
    grant["granted_permissions"] = json!(["workspace.edit", "workspace.check_revision"]);
    f.write("check-grant.json", grant);

    let adapters = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../adapters");
    let checked_schemas = adapters.join("workspace-check/schemas");
    let mut host = f.read("host.json");
    host["schemas"] = json!([
        adapters.join("example-reasoner/schemas/workspace-repair-task.schema.json"),
        checked_schemas.join("candidate.schema.json"),
        checked_schemas.join("execution.schema.json"),
        checked_schemas.join("edit-candidate.schema.json"),
        checked_schemas.join("edit-execution.schema.json"),
        checked_schemas.join("revision-candidate.schema.json"),
        checked_schemas.join("revision-execution.schema.json"),
        "edit-success.schema.json",
        "check-success.schema.json",
    ]);
    f.write("host.json", host);
    f.write(
        "edit-success.schema.json",
        success_schema(
            "arcs.policy.repair_edit_success.v1",
            json!({
                "status":{"enum":["applied","unchanged"]},"path":{"const":"calculator.py"},
                "commit":{"type":"string","minLength":40}
            }),
            &["status", "path", "commit"],
        ),
    );
    f.write(
        "check-success.schema.json",
        success_schema(
            "arcs.policy.repair_check_success.v1",
            json!({
                "status":{"const":"passed"},"check":{"const":"unit"},"exit_code":{"const":0},
                "revision":{"type":"string","minLength":40}
            }),
            &["status", "check", "exit_code", "revision"],
        ),
    );

    let mut runtime = f.read("runtime.json");
    runtime["runtime_id"] = json!("workspace-repair");
    let mut entry = runtime["routes"][0].clone();
    let mut template = entry.clone();
    let fields = json!(["check", "path", "base_commit", "replacement"]);
    for route in [&mut entry, &mut template] {
        route["input_schema_id"] = json!(TASK);
        route["payload_fields"] = fields.clone();
    }
    entry.as_object_mut().unwrap().remove("goal");
    entry["id"] = json!("repair");
    entry["objective"] = json!("Plan the requested repair and verify the published Git revision.");
    entry["target_schema_id"] = json!("arcs.plan_candidate.v1");
    entry["allowed_capabilities"] = json!([]);
    entry["max_reasoning_requests"] = json!(5);

    template
        .as_object_mut()
        .unwrap()
        .remove("max_reasoning_requests");
    template["goal"]
        .as_object_mut()
        .unwrap()
        .remove("between_rounds");
    template["goal"]["max_rounds"] = json!(1);
    let mut edit = template.clone();
    let mut check = template;
    for (route, name, capability, candidate, success) in [
        (
            &mut edit,
            "edit",
            "edit.apply",
            EDIT_CANDIDATE,
            "arcs.policy.repair_edit_success.v1",
        ),
        (
            &mut check,
            "check",
            "check.revision",
            CHECK_CANDIDATE,
            "arcs.policy.repair_check_success.v1",
        ),
    ] {
        let executor = json!({"adapter_id":"workspace.checker","capability_id":capability});
        route["id"] = json!(name);
        route["objective"] = json!(if name == "edit" {
            "Propose the requested file repair relative to its Git base."
        } else {
            "Check the exact revision confirmed by the edit dependency."
        });
        route["target_schema_id"] = json!(candidate);
        route["allowed_capabilities"] = json!([executor]);
        route["goal"]["actions"] =
            json!([{"capability":executor,"verification_schema_id":candidate}]);
        route["goal"]["execution_success_schema_id"] = json!(success);
        route["goal"]["assessment"]["objective"] =
            json!("Assess the receipt against the requested action.");
    }
    edit["goal"]["export_execution_schema_id"] = json!(EDIT_RECEIPT);
    entry["plan"] = json!({"max_steps":2,"max_total_runtime_ms":5000,"max_total_actions":2,
        "templates":[{"route":edit,"export_fields":[export]},{"route":check,"export_fields":["goal_status"]}]});
    runtime["routes"] = json!([entry]);
    f.write("runtime.json", runtime);
    base
}

fn success_schema(id: &str, fields: Value, required: &[&str]) -> Value {
    json!({
        "$id":id,"type":"object","required":["action_version","result"],"additionalProperties":false,"properties":{
            "action_version":{"type":"string","minLength":1},
            "result":{"type":"object","required":required,"properties":fields}
        }
    })
}

fn advance<T>(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    now: u64,
    ready: impl Fn(&TaskRuntime) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        runtime.tick_at(host, now).unwrap();
        if let Some(value) = ready(runtime) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}",
            runtime.report(0).unwrap()
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}

#[test]
fn an_edit_plan_hands_its_exact_receipt_to_the_separately_approved_revision_check() {
    let f = Fixture::new();
    let base = configure(&f, "result");
    let (mut host, mut runtime) = f.open();
    let edit = until(&mut host, &mut runtime, NOW, |state| {
        state == GoalState::AwaitingApproval
    });
    assert_eq!(edit.route_id, "p1.edit");
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), base);
    assert_eq!(f.checks(), 0);
    approval(&host, &mut runtime, &edit, edit.candidate.as_ref().unwrap()).unwrap();
    let edit_done = until(&mut host, &mut runtime, NOW, |state| {
        state == GoalState::Completed
    });
    let edit_execution = edit_done.execution.as_ref().unwrap();
    let edit_result = result(&host, Some(edit_execution));
    let commit = edit_result["result"]["commit"].as_str().unwrap();
    assert_ne!(commit, base);
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), commit);
    assert_eq!(f.checks(), 0);
    host.shutdown().unwrap();
    drop((host, runtime));

    let later = git(
        &f,
        &[
            "commit-tree",
            "HEAD^{tree}",
            "-p",
            commit,
            "-m",
            "A competing project revision",
        ],
    );
    git(&f, &["update-ref", REF, later.trim(), commit]);

    let (mut host, mut runtime) = f.open();
    let check = advance(&mut host, &mut runtime, NOW + MONTH, |r| {
        r.goals_after(0, 10)
            .unwrap()
            .into_iter()
            .find(|goal| goal.route_id == "p1.check" && goal.state == GoalState::AwaitingApproval)
    });
    let bindings = runtime.dependencies_after(0, 10).unwrap();
    let binding = &bindings[0].bindings[0];
    assert_eq!(binding.result_version_id, edit_done.assessment.unwrap());
    assert_eq!(binding.context[0].version_id, *edit_execution);
    assert_eq!(binding.context[0].payload_fields, ["result"]);
    let check_candidate = check.candidate.as_ref().unwrap();
    let candidate = host.store().get(check_candidate).unwrap().unwrap();
    assert_eq!(candidate.payload, json!({"check":"unit","revision":commit}));
    assert!(
        candidate
            .provenance
            .unwrap()
            .parents
            .contains(&edit_execution.0)
    );
    assert!(
        approval(
            &host,
            &mut runtime,
            &check,
            edit.candidate.as_ref().unwrap()
        )
        .is_err()
    );
    assert_eq!(f.checks(), 0);
    approval(&host, &mut runtime, &check, check_candidate).unwrap();
    advance(&mut host, &mut runtime, NOW + MONTH, |r| {
        (r.plans_after(0, 1).unwrap()[0].state == PlanState::ReadyForReview).then_some(())
    });
    let done = runtime.goals_after(0, 10).unwrap().pop().unwrap();
    let receipt = host
        .store()
        .get(done.execution.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(receipt.schema_id.0, CHECK_RECEIPT);
    assert_eq!(receipt.payload["result"]["revision"], commit);
    assert_eq!(receipt.payload["result"]["status"], "passed");
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), later.trim());
    assert_eq!(done.state, GoalState::Completed);
    assert_eq!(f.checks(), 1);
    assert_eq!(
        fs::read_to_string(f.path("project/calculator.py")).unwrap(),
        BROKEN
    );
    host.shutdown().unwrap();
}

#[test]
fn a_conflicting_edit_blocks_its_revision_check() {
    let f = Fixture::new();
    let base = configure(&f, "result");
    let (mut host, mut runtime) = f.open();
    let edit = until(&mut host, &mut runtime, NOW, |state| {
        state == GoalState::AwaitingApproval
    });
    git(&f, &["commit", "--allow-empty", "-qm", "New project base"]);
    let newer = git(&f, &["rev-parse", "HEAD"]);
    git(&f, &["update-ref", REF, newer.trim(), &base]);
    approval(&host, &mut runtime, &edit, edit.candidate.as_ref().unwrap()).unwrap();
    advance(&mut host, &mut runtime, NOW, |r| {
        (r.plans_after(0, 1).unwrap()[0].state == PlanState::Blocked).then_some(())
    });
    let goals = runtime.goals_after(0, 10).unwrap();
    assert_eq!(goals[0].state, GoalState::Blocked);
    assert_eq!(
        result(&host, goals[0].execution.as_ref())["result"]["status"],
        "conflict"
    );
    assert!(
        goals
            .iter()
            .all(|goal| goal.route_id != "p1.check" || goal.candidate.is_none())
    );
    assert_eq!(f.checks(), 0);
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), newer.trim());
    host.shutdown().unwrap();
}

#[test]
fn a_plan_cannot_request_a_receipt_field_outside_the_template_exports() {
    let f = Fixture::new();
    let base = configure(&f, "action_version");
    let (mut host, mut runtime) = f.open();
    let detail = advance(&mut host, &mut runtime, NOW, |r| {
        r.plans_after(0, 1)
            .unwrap()
            .pop()
            .filter(|plan| plan.state == PlanState::Rejected)
            .and_then(|plan| plan.detail)
    });
    assert!(detail.contains("outside the template exports"), "{detail}");
    assert!(runtime.goals_after(0, 10).unwrap().is_empty());
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), base);
    assert_eq!(f.checks(), 0);
    host.shutdown().unwrap();
}

fn configure_retry(f: &Fixture, share_failure: bool) {
    configure(f, "result");
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["replacement"] =
        json!("def add(left, right):\n    return left * right\n");
    f.write("input-profile.json", input);

    fs::write(
        f.path("retry.py"),
        include_str!("../support/workspace_retry_peer.py"),
    )
    .unwrap();
    f.write("retry.json", json!({"replacement":FIXED}));
    let mut grant = f.read("reasoner-grant.json");
    grant["adapter_id"] = json!("workspace.replanner");
    f.write("retry-grant.json", grant);
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({
        "program":"/usr/bin/python3","args":[{"path":"retry.py"},{"path":"retry.json"}],
        "grant_file":"retry-grant.json","timeout_ms":10000
    }));
    f.write("host.json", host);

    let capability = json!({"adapter_id":"workspace.replanner","capability_id":"solution.propose"});
    let mut runtime = f.read("runtime.json");
    let entry = &mut runtime["routes"][0];
    entry["target_schema_id"] = json!("arcs.plan_candidate.v2");
    entry["reasoner"] = capability.clone();
    entry["max_reasoning_requests"] = json!(10);
    entry["plan"]["max_total_runtime_ms"] = json!(12_000);
    entry["plan"]["max_total_actions"] = json!(4);
    entry["plan"]["replanning"] = json!({"max_revisions":1,"reuse_completed_exports":true});
    let templates = entry["plan"]["templates"].as_array_mut().unwrap();
    for template in templates.iter_mut() {
        template["route"]["goal"]["exclusive_resources"] = json!(["workspace-repair"]);
    }
    templates[1]["route"]["goal"]["export_execution_schema_id"] = json!(CHECK_RECEIPT);
    templates[1]["export_fields"] = json!(["result"]);
    if share_failure {
        templates[1]["replanning_failure_receipt_fields"] = json!(["result"]);
    }
    let mut retries = templates.clone();
    for (template, name) in retries.iter_mut().zip(["retry-edit", "retry-check"]) {
        template["route"]["id"] = json!(name);
        template["route"]["reasoner"] = capability.clone();
    }
    templates.extend(retries);
    f.write("runtime.json", runtime);
}

fn awaiting_action(host: &mut Host, runtime: &mut TaskRuntime, now: u64, route: &str) -> GoalTask {
    advance(host, runtime, now, |runtime| {
        runtime
            .goals_after(0, 10)
            .unwrap()
            .into_iter()
            .find(|goal| goal.route_id == route && goal.state == GoalState::AwaitingApproval)
    })
}

fn run_failed_revision(host: &mut Host, runtime: &mut TaskRuntime) -> GoalTask {
    for route in ["p1.r0.edit", "p1.r0.check"] {
        let task = awaiting_action(host, runtime, NOW, route);
        approval(host, runtime, &task, task.candidate.as_ref().unwrap()).unwrap();
    }
    advance(host, runtime, NOW, |runtime| {
        (runtime.plans_after(0, 1).unwrap()[0].state == PlanState::Replanning).then_some(())
    });
    let failed = runtime.goals_after(0, 10).unwrap().pop().unwrap();
    assert_eq!(failed.route_id, "p1.r0.check");
    assert_eq!(failed.state, GoalState::Blocked);
    assert_eq!(result(host, failed.outcome.as_ref())["verdict"], "failure");
    failed
}

#[test]
fn a_failed_revision_receipt_drives_a_replacement_plan_that_repairs_and_rechecks_the_code() {
    let f = Fixture::new();
    configure_retry(&f, true);
    let (mut host, mut runtime) = f.open();
    let failed = run_failed_revision(&mut host, &mut runtime);
    let failed_receipt = failed.execution.as_ref().unwrap();
    let failed_result = result(&host, Some(failed_receipt));
    let bad_commit = failed_result["result"]["revision"].as_str().unwrap();
    assert_eq!(failed_result["result"]["status"], "failed");
    assert_eq!(f.checks(), 1);
    host.shutdown().unwrap();
    drop((host, runtime));

    let (mut host, mut runtime) = f.open();
    let retry = awaiting_action(&mut host, &mut runtime, NOW + MONTH, "p1.r1.edit");
    let plan = runtime.plans_after(0, 1).unwrap().remove(0);
    let context_id = plan.revisions[1].context_version_id.as_ref().unwrap();
    let context = host.store().get(context_id).unwrap().unwrap();
    assert_eq!(context.schema_id.0, "arcs.plan_replanning.v2");
    let summary = &context.payload["steps"][1];
    assert_eq!(summary["resolution"], "blocked");
    assert_eq!(summary["output_version_ids"], json!([]));
    assert_eq!(summary["failure_receipt_version_id"], json!(failed_receipt));
    assert_eq!(
        context.payload["context"][0],
        json!({"version_id":failed_receipt,"payload_fields":["result"]})
    );
    assert_eq!(
        result(&host, retry.candidate.as_ref())["base_commit"],
        bad_commit
    );
    assert_eq!(git(&f, &["rev-parse", REF]).trim(), bad_commit);
    assert_eq!(f.checks(), 1);
    assert!(
        approval(
            &host,
            &mut runtime,
            &retry,
            failed.candidate.as_ref().unwrap()
        )
        .is_err()
    );

    approval(
        &host,
        &mut runtime,
        &retry,
        retry.candidate.as_ref().unwrap(),
    )
    .unwrap();
    let check = awaiting_action(&mut host, &mut runtime, NOW + MONTH, "p1.r1.check");
    let fixed_commit = git(&f, &["rev-parse", REF]).trim().to_owned();
    assert_ne!(fixed_commit, bad_commit);
    assert_eq!(
        result(&host, check.candidate.as_ref())["revision"],
        fixed_commit
    );
    assert_eq!(f.checks(), 1);
    approval(
        &host,
        &mut runtime,
        &check,
        check.candidate.as_ref().unwrap(),
    )
    .unwrap();
    advance(&mut host, &mut runtime, NOW + MONTH, |runtime| {
        (runtime.plans_after(0, 1).unwrap()[0].state == PlanState::ReadyForReview).then_some(())
    });
    let checked = runtime.goals_after(0, 10).unwrap().pop().unwrap();
    let receipt = result(&host, checked.execution.as_ref());
    assert_eq!(receipt["result"]["status"], "passed");
    assert_eq!(receipt["result"]["revision"], fixed_commit);
    assert_eq!(f.checks(), 2);
    assert_eq!(
        fs::read_to_string(f.path("project/calculator.py")).unwrap(),
        BROKEN
    );
    host.shutdown().unwrap();
}

#[test]
fn a_failed_receipt_stays_private_without_a_template_export() {
    let f = Fixture::new();
    configure_retry(&f, false);
    let (mut host, mut runtime) = f.open();
    let failed = run_failed_revision(&mut host, &mut runtime);
    advance(&mut host, &mut runtime, NOW, |runtime| {
        let plan = runtime.plans_after(0, 1).unwrap().remove(0);
        matches!(plan.state, PlanState::Blocked | PlanState::Rejected).then_some(())
    });
    let db = rusqlite::Connection::open(f.path("retry.sqlite")).unwrap();
    let input: String = db
        .query_row(
            "SELECT input FROM calls ORDER BY rowid DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let invocation: Value = serde_json::from_str(&input).unwrap();
    assert_eq!(invocation["constraints"]["task_replanning"]["revision"], 1);
    assert!(!input.contains(&failed.execution.unwrap().0));
    assert_eq!(f.checks(), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_restarted_replanner_rejects_receipt_fields_outside_the_frozen_scope() {
    let f = Fixture::new();
    configure_retry(&f, true);
    let (mut host, mut runtime) = f.open();
    let failed = run_failed_revision(&mut host, &mut runtime);
    host.shutdown().unwrap();
    drop((host, runtime));

    let db = rusqlite::Connection::open(f.path("runtime.sqlite")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT context_json FROM runtime_plan_replanning",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut context: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(context["context"][0]["version_id"], json!(failed.execution));
    context["context"][0]["payload_fields"] = json!(["result", "action_version"]);
    db.execute(
        "UPDATE runtime_plan_replanning SET context_json=?1",
        [context.to_string()],
    )
    .unwrap();

    let (mut host, mut runtime) = f.open();
    advance(&mut host, &mut runtime, NOW + MONTH, |runtime| {
        let plan = runtime.plans_after(0, 1).unwrap().remove(0);
        (plan.state == PlanState::Blocked).then_some(())
    });
    let planner = rusqlite::Connection::open(f.path("retry.sqlite")).unwrap();
    let calls: i64 = planner
        .query_row("SELECT count(*) FROM calls", [], |row| row.get(0))
        .unwrap();
    assert_eq!(calls, 1);
    let core = rusqlite::Connection::open(f.path("core.sqlite")).unwrap();
    let publications: i64 = core
        .query_row(
            "SELECT count(*) FROM artifact_versions WHERE schema_id='arcs.plan_replanning.v2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(publications, 0);
    assert_eq!(f.checks(), 1);
    host.shutdown().unwrap();
}

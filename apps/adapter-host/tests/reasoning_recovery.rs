#[path = "support/goals.rs"]
mod goal_support;
#[path = "support/parallel.rs"]
mod parallel_support;
#[path = "support/plans.rs"]
mod plan_support;
mod support;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs::reasoning::recovery::{SCHEMA, STEP_SCHEMA};
use arcs_host::{
    Host,
    task_runtime::{
        RuntimeConfig, TaskRuntime,
        goal::GoalState,
        suspension::{TaskChange, TaskControlCommand},
    },
};
use parallel_support::Fixture;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn choice(route: &Value, adapter: &str) -> Value {
    json!({"capability":{"adapter_id":adapter,"capability_id":"solution.propose"},"method":route["method"],"budget":route["budget"]})
}
fn recover(route: &mut Value, adapter: &str, delay: u64) {
    route["reasoning_recovery"] = json!({"alternatives":[choice(route,adapter)],"delay_ms":delay});
}
fn simple(a: &str, b: &str) -> Fixture {
    let f = Fixture::new();
    f.peer("a", a, false);
    f.peer("b", b, false);
    f.route("main", "a");
    let mut config = f.read("runtime.json");
    recover(&mut config["routes"][0], "b", 0);
    f.write("runtime.json", config);
    f
}
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    now: Option<u64>,
    condition: impl Fn(&Host, &TaskRuntime) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(now) = now {
            runtime.tick_at(host, now).unwrap();
        } else {
            runtime.tick(host).unwrap();
        }
        if condition(host, runtime) {
            return;
        }
        assert!(Instant::now() < end, "{:?}", runtime.report(0).unwrap());
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn artifacts(host: &Host, schema: &str) -> Vec<arcs::core::Artifact> {
    plan_support::artifacts(host, schema)
}
fn failures(host: &Host) -> usize {
    artifacts(host, arcs::reasoning::attempts::SCHEMA)
        .iter()
        .filter(|a| a.payload["outcome"]["kind"] == "failed")
        .count()
}

#[test]
fn actual_fallback_preserves_the_initial_selection_and_independent_work() {
    let f = simple("fail", "normal");
    f.route("independent", "b");
    let mut config = f.read("runtime.json");
    let first = config["routes"][0].clone();
    config["routes"][0]["reasoner_selection"] = json!({"alternatives":[choice(&first,"b")]});
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().ready_for_review == 2
    });
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 0);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 2);
    let selections = runtime.reasoner_selections(&host).unwrap();
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].selected.capability.adapter_id.0, "a");
    assert_eq!(artifacts(&host, SCHEMA).len(), 1);
    assert_eq!(artifacts(&host, STEP_SCHEMA).len(), 2);
    let completed = runtime.completed_after(0, 10).unwrap();
    assert!(completed.iter().all(|result| {
        host.store()
            .get(&result.candidate_version_ids[0])
            .unwrap()
            .unwrap()
            .created_by
            .id
            == "b"
    }));
    let counts = (f.count("a", "attempts"), f.count("b", "attempts"));
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(counts, (f.count("a", "attempts"), f.count("b", "attempts")));
    host.shutdown().unwrap();
}

#[test]
fn permanent_invalid_answers_and_no_solution_do_not_trigger_fallback() {
    for mode in ["invalid", "no_solution"] {
        let f = simple(mode, "normal");
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, None, |_, r| {
            let report = r.report(0).unwrap();
            report.failed_jobs + report.ready_for_review == 1
        });
        assert_eq!(f.count("a", "attempts"), 1);
        assert_eq!(f.count("b", "attempts"), 0);
        assert_eq!(artifacts(&host, STEP_SCHEMA).len(), 1);
        host.shutdown().unwrap();
    }
}

#[test]
fn due_time_and_failed_request_survive_restart_without_retrying_the_old_backend() {
    let f = simple("fail", "normal");
    let mut config = f.read("runtime.json");
    config["routes"][0]["reasoning_recovery"]["delay_ms"] = json!(5000);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, Some(1000), |h, _| failures(h) == 1);
    assert_eq!(f.count("b", "attempts"), 0);
    let steps = artifacts(&host, STEP_SCHEMA);
    let due = steps
        .iter()
        .find_map(|a| a.payload["not_before_unix_ms"].as_u64().filter(|n| *n > 0))
        .unwrap();
    assert_eq!(due, 6000);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for now in [1001, 5999, 500, 5999] {
        runtime.tick_at(&mut host, now).unwrap();
    }
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 0);
    until(&mut host, &mut runtime, Some(due), |_, r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn an_exhausted_chain_and_a_lost_operational_failure_checkpoint_cannot_extend_the_budget() {
    let f = simple("fail", "fail");
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().failed_jobs == 1
    });
    assert_eq!(failures(&host), 2);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let db = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    db.execute("UPDATE runtime_jobs SET state='running',error=NULL", [])
        .unwrap();
    drop(db);
    let (mut host, mut runtime) = f.open();
    for _ in 0..6 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 1);
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_paused_failed_worker_waits_for_resume_before_selecting_or_dispatching_an_alternative() {
    let f = simple("fail", "normal");
    let path = f.directory.join("peer.py");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path,source.replace("  if c.get('mode')=='fail':","  if c.get('mode')=='fail':\n   (root/(c['id']+'.started')).write_text(key)\n   while not (root/(c['id']+'.release')).exists(): time.sleep(.003)")).unwrap();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, _| f.started("a"));
    let command = |revision, change| TaskControlCommand {
        sequence: 1,
        expected_revision: revision,
        command_id: format!("control-{revision}"),
        operator: Actor {
            actor_type: ActorType::Human,
            id: "test.operator".into(),
        },
        reason: "test pause and resume".into(),
        change,
    };
    runtime
        .control_task(&mut host, command(0, TaskChange::Pause))
        .unwrap();
    f.release("a");
    until(&mut host, &mut runtime, None, |h, _| failures(h) == 1);
    assert_eq!(runtime.report(0).unwrap().paused_jobs, 1);
    assert_eq!(runtime.report(0).unwrap().failed_jobs, 0);
    assert_eq!(artifacts(&host, STEP_SCHEMA).len(), 1);
    assert_eq!(f.count("b", "attempts"), 0);
    runtime
        .control_task(
            &mut host,
            command(
                1,
                TaskChange::Resume {
                    recover_dispatched: false,
                },
            ),
        )
        .unwrap();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn generated_steps_join_the_actual_fallback_producer_and_include_all_recovery_budgets() {
    let f = plan_support::fixture("fail", "normal");
    f.peer("backup", "normal", false);
    let mut config = f.read("runtime.json");
    recover(
        &mut config["routes"][0]["plan"]["templates"][0]["route"],
        "backup",
        0,
    );
    config["routes"][0]["plan"]["max_total_runtime_ms"] = json!(40000);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("backup", "attempts"), 1);
    assert_eq!(f.count("join", "attempts"), 1);
    plan_support::assert_no_authority(&host);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(f.count("backup", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn invalid_recovery_configuration_and_unbudgeted_plans_are_rejected_before_dispatch() {
    for mode in [
        "empty",
        "too_many",
        "delay",
        "grant",
        "reserved",
        "plan_budget",
    ] {
        let f = if mode == "plan_budget" {
            plan_support::fixture("normal", "normal")
        } else {
            simple("normal", "normal")
        };
        let mut config = f.read("runtime.json");
        if mode == "plan_budget" {
            recover(
                &mut config["routes"][0]["plan"]["templates"][0]["route"],
                "b",
                0,
            );
        } else {
            match mode {
                "empty" => config["routes"][0]["reasoning_recovery"]["alternatives"] = json!([]),
                "too_many" => {
                    let item = config["routes"][0]["reasoning_recovery"]["alternatives"][0].clone();
                    config["routes"][0]["reasoning_recovery"]["alternatives"] =
                        json!(vec![item; 8]);
                }
                "delay" => config["routes"][0]["reasoning_recovery"]["delay_ms"] = json!(60001),
                "grant" => {
                    config["routes"][0]["reasoning_recovery"]["alternatives"][0]["budget"]["max_runtime_ms"] =
                        json!(30001)
                }
                _ => {
                    config["routes"][0]["constraints"]["arcs_reasoning_recovery"] = json!("forged")
                }
            }
        }
        f.write("runtime.json", config);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
        if mode == "plan_budget" {
            let mut runtime = TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
            )
            .unwrap();
            until(&mut host, &mut runtime, None, |_, r| {
                r.report(0).unwrap().stopped_plans == 1
            });
            assert_eq!(f.count("a", "attempts"), 0);
            assert_eq!(f.count("b", "attempts"), 0);
        } else {
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
}

fn recovering_planner() -> Fixture {
    let f = plan_support::fixture("normal", "normal");
    std::fs::write(
        f.directory.join("backup-planner.py"),
        include_str!("support/planner_peer.py"),
    )
    .unwrap();
    let mut backup = f.read("planner.json");
    backup["adapter_id"] = json!("backup-planner");
    f.write("backup-planner.json", backup);
    let mut primary = f.read("planner.json");
    primary["unavailable"] = json!(true);
    primary["unavailable_message"] = json!("planner unavailable");
    f.write("planner.json", primary);
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("backup-planner");
    f.write("backup-planner-grant.json", grant);
    let mut host = f.read("host.json");
    host["adapters"].as_array_mut().unwrap().push(json!({"program":"/usr/bin/python3","args":[{"path":"backup-planner.py"},{"path":"backup-planner.json"}],"grant_file":"backup-planner-grant.json","timeout_ms":15000}));
    f.write("host.json", host);
    let mut config = f.read("runtime.json");
    recover(&mut config["routes"][0], "backup-planner", 0);
    f.write("runtime.json", config);
    f
}

#[test]
fn a_fallback_planner_origin_is_checked_when_loading_its_immutable_children_after_restart() {
    let f = recovering_planner();
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().review_plans == 1
    });
    assert_eq!(f.count("planner", "attempts"), 1);
    assert_eq!(f.count("backup-planner", "attempts"), 1);
    let status = runtime.reasoning_recoveries(&host).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(
        status[0].recovery.selected.capability.adapter_id.0,
        "backup-planner"
    );
    assert_eq!(status[0].recovery.attempt_number, 2);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    for _ in 0..4 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(runtime.report(0).unwrap().review_plans, 1);
    assert_eq!(f.count("backup-planner", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn plan_cancellation_and_lifetime_expiry_prevent_a_delayed_alternative() {
    for expiry in [false, true] {
        let f = recovering_planner();
        let mut config = f.read("runtime.json");
        config["routes"][0]["reasoning_recovery"]["delay_ms"] = json!(5000);
        if expiry {
            config["routes"][0]["max_lifetime_ms"] = json!(1000);
        }
        f.write("runtime.json", config);
        let (mut host, mut runtime) = f.open();
        until(&mut host, &mut runtime, Some(1000), |h, _| failures(h) == 1);
        assert_eq!(f.count("backup-planner", "attempts"), 0);
        if expiry {
            runtime.tick_at(&mut host, 2000).unwrap();
            assert_eq!(runtime.report(0).unwrap().expired_jobs, 1);
        } else {
            runtime
                .cancel_plan(&mut host, 1, "cancel delayed recovery".into())
                .unwrap();
        }
        for _ in 0..4 {
            runtime.tick_at(&mut host, 10000).unwrap();
        }
        assert_eq!(f.count("planner", "attempts"), 1);
        assert_eq!(f.count("backup-planner", "attempts"), 0);
        host.shutdown().unwrap();
    }
}

#[test]
fn a_crash_between_core_failure_and_recovery_publication_keeps_one_failed_backend_call() {
    let f = simple("fail", "normal");
    let (mut host, mut runtime) = f.open();
    let core = rusqlite::Connection::open(f.directory.join("core.sqlite")).unwrap();
    core.execute_batch("CREATE TRIGGER fail_recovery BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.reasoning_recovery_step.v1' AND json_extract(NEW.artifact_json,'$.payload.index')=1 BEGIN SELECT RAISE(ABORT,'interrupted recovery publication'); END;").unwrap();
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    journal.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON runtime_jobs WHEN NEW.state='failed' BEGIN SELECT RAISE(ABORT,'lost failure checkpoint'); END;").unwrap();
    let end = Instant::now() + Duration::from_secs(25);
    loop {
        if runtime.tick(&mut host).is_err() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(failures(&host), 1);
    assert_eq!(artifacts(&host, STEP_SCHEMA).len(), 1);
    assert_eq!(f.count("b", "attempts"), 0);
    core.execute_batch("DROP TRIGGER fail_recovery").unwrap();
    journal
        .execute_batch("DROP TRIGGER fail_checkpoint")
        .unwrap();
    drop(core);
    drop(journal);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().ready_for_review == 1
    });
    assert_eq!(f.count("a", "attempts"), 1);
    assert_eq!(f.count("b", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn recovered_goal_rounds_keep_their_own_evidence_approvals_and_confirmed_effects() {
    let f = goal_support::Fixture::new();
    f.manual();
    let mut host_config = f.read("host.json");
    let mut profile = f.read("planner-profile.json");
    profile["adapter_id"] = json!("goal.backup");
    f.write("backup-profile.json", profile);
    let mut grant = f.read("planner-grant.json");
    grant["adapter_id"] = json!("goal.backup");
    f.write("backup-grant.json", grant);
    let mut adapter = host_config["adapters"][2].clone();
    adapter["profile"] = json!("backup-profile.json");
    adapter["grant_file"] = json!("backup-grant.json");
    adapter["args"] = json!([{"path":f.directory.join("backup.sqlite")}]);
    host_config["adapters"]
        .as_array_mut()
        .unwrap()
        .push(adapter);
    let script = r#"import json,sys
for line in sys.stdin:
 p=json.loads(line);m=p['message'];kind=m['type']
 if kind=='hello':reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'goal.planner','adapter_version':'0.1.0','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.fan_candidate.v1','arcs.goal_assessment_candidate.v1']},'required_permissions':['reason.compute']}]}}
 elif kind=='reason_v2':
  with open(sys.argv[1],'a') as calls:calls.write(m['invocation']['invocation_id']+'\n')
  reply={'type':'error','code':'unavailable','message':'planner failed','retryable':True}
 elif kind=='shutdown':reply={'type':'bye'}
 else:raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
"#;
    std::fs::write(f.directory.join("failed-planner.py"), script).unwrap();
    host_config["adapters"][2] = json!({"program":"/usr/bin/python3","args":[{"path":"failed-planner.py"},{"path":"planner-calls.txt"}],"grant_file":"planner-grant.json","timeout_ms":15000});
    f.write("host.json", host_config);
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(2);
    config["routes"][0]["goal"]["max_rounds"] = json!(2);
    recover(&mut config["routes"][0], "goal.backup", 0);
    f.write("runtime.json", config);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":false,"minimum_actuations":2});
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().awaiting_approval == 1
    });
    let goal = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(goal.state, GoalState::AwaitingApproval);
    let candidate = host
        .store()
        .get(goal.candidate.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(candidate.created_by.id, "goal.backup");
    let count = || {
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count(), 0);
    let journal = rusqlite::Connection::open(f.directory.join("runtime.sqlite")).unwrap();
    let mut different_round = serde_json::to_value(&goal).unwrap();
    different_round["round"] = json!(2);
    journal
        .execute(
            "UPDATE runtime_jobs SET goal_json=?1 WHERE sequence=1",
            [different_round.to_string()],
        )
        .unwrap();
    assert!(
        runtime
            .review_goal(
                &host,
                goal.sequence,
                goal.candidate.as_ref().unwrap(),
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::Human,
                    id: "test.operator".into()
                },
                "old candidate in another round".into()
            )
            .is_err()
    );
    assert_eq!(count(), 0);
    journal
        .execute(
            "UPDATE runtime_jobs SET goal_json=?1 WHERE sequence=1",
            [serde_json::to_string(&goal).unwrap()],
        )
        .unwrap();
    drop(journal);
    runtime
        .review_goal(
            &host,
            goal.sequence,
            goal.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "test.operator".into(),
            },
            "reviewed recovered proposal".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, None, |_, r| {
        r.goals_after(0, 1)
            .unwrap()
            .first()
            .is_some_and(|goal| goal.round == 2 && goal.state == GoalState::AwaitingApproval)
    });
    assert_eq!(count(), 1);
    let next = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_ne!(next.candidate, goal.candidate);
    runtime
        .review_goal(
            &host,
            next.sequence,
            next.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "test.operator".into(),
            },
            "reviewed next recovered round".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, None, |_, r| {
        r.report(0).unwrap().completed_goals == 1
    });
    assert_eq!(count(), 2);
    assert_eq!(
        artifacts(&host, SCHEMA).len(),
        2,
        "only planning rounds should have recovery definitions"
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..3 {
        runtime.tick(&mut host).unwrap();
    }
    assert_eq!(count(), 2);
    assert_eq!(
        std::fs::read_to_string(f.directory.join("planner-calls.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn recovery_planners_need_their_own_binary_input_receiver_grants() {
    let f = goal_support::Fixture::new();
    let mut config = f.read("runtime.json");
    recover(&mut config["routes"][0], "goal.planner", 0);
    config["routes"][0]["goal"]["sources"][0]["binary"] = json!({"name":"frame","source":{"adapter_id":"demo.fan","capability_id":"fan.read"},"reference_pointer":"/frame"});
    f.write("runtime.json", config);
    let mut host = f.host();
    let error = match TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("runtime.json")).unwrap(),
    ) {
        Ok(_) => panic!("missing binary receiver accepted"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("binary input receiver"),
        "{error}"
    );
    assert!(!f.directory.join("runtime.sqlite").exists());
    host.shutdown().unwrap();
}

#[test]
fn single_worker_recovery_delay_starts_after_the_failed_call_finishes() {
    let f = simple("fail", "normal");
    let mut config = f.read("runtime.json");
    config["max_concurrent_reasoning"] = json!(1);
    config["routes"][0]["reasoning_recovery"]["delay_ms"] = json!(2000);
    f.write("runtime.json", config);
    let path = f.directory.join("peer.py");
    let script = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path,script.replace("  if c.get('mode')=='fail':","  if c.get('mode')=='fail':\n   time.sleep(.2)\n   (root/'failure-clock').write_text(str(time.time_ns()//1000000))")).unwrap();
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    until(&mut host, &mut runtime, None, |h, _| failures(h) == 1);
    assert_eq!(failures(&host), 1);
    let failed_at = std::fs::read_to_string(f.directory.join("failure-clock"))
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let next = artifacts(&host, STEP_SCHEMA)
        .into_iter()
        .find(|a| a.payload["index"] == 1)
        .unwrap();
    let due = next.payload["not_before_unix_ms"].as_u64().unwrap();
    assert!(
        due >= failed_at + 2000,
        "delay began before failure completion: {due} vs {failed_at}"
    );
    assert_eq!(f.count("b", "attempts"), 0);
    host.shutdown().unwrap();
}

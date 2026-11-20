#[path = "support/goals.rs"]
mod goals;
#[path = "support/requests.rs"]
mod requests;
mod support;
use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType};
use arcs_host::task_runtime::goal::GoalState;
use arcs_host::task_runtime::suspension::{TaskChange, TaskControlCommand};
use requests::*;
use serde_json::json;

#[test]
fn a_gated_request_does_not_block_two_other_goals_or_sensor_polling() {
    let f = fixture();
    peer(&f, "slow", "gate");
    peer(&f, "fast", "ready");
    peer(&f, "other", "ready");
    routes(&f, &["slow", "fast", "other"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        let report = rt.report(0).unwrap();
        started(&f, "slow") && report.awaiting_approval == 2
    });
    let pending = host.request_workers();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].capability.adapter_id.0, "slow");
    assert!(!pending[0].discarded);
    assert!(
        host.report()
            .unwrap()
            .adapters
            .iter()
            .any(|a| a.observations_committed > 0)
    );
    let slow = runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.route_id == "slow")
        .unwrap();
    assert_eq!(slow.state, GoalState::Acquiring);
    assert_eq!(slow.wait_ticks, 0);
    release(&f, "slow");
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 3
    });
    for id in ["slow", "fast", "other"] {
        assert_eq!(count(&f, id, "attempts"), 1);
    }
    host.shutdown().unwrap();
}
#[test]
fn request_pool_backpressure_keeps_the_third_peer_idle_until_a_slot_is_free() {
    let f = fixture();
    for id in ["a", "b", "c"] {
        peer(&f, id, "gate");
    }
    routes(&f, &["a", "b", "c"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| {
        started(&f, "a") && started(&f, "b")
    });
    for _ in 0..8 {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert!(!started(&f, "c"));
    assert_eq!(host.request_workers().len(), 2);
    release(&f, "a");
    until(&mut host, &mut runtime, |_, _| started(&f, "c"));
    assert!(!f.directory.join("b.release").exists());
    release(&f, "b");
    release(&f, "c");
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 3
    });
    for id in ["a", "b", "c"] {
        assert_eq!(count(&f, id, "attempts"), 1);
    }
    host.shutdown().unwrap();
}
#[test]
fn a_lost_response_reuses_the_exact_request_and_the_peer_journal_after_restart() {
    let f = fixture();
    peer(&f, "source", "gate");
    routes(&f, &["source"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| started(&f, "source"));
    let key = std::fs::read_to_string(f.directory.join("source.started")).unwrap();
    assert_eq!(count(&f, "source", "results"), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    release(&f, "source");
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    assert_eq!(
        std::fs::read_to_string(f.directory.join("source.started")).unwrap(),
        key
    );
    assert_eq!(count(&f, "source", "attempts"), 2);
    assert_eq!(count(&f, "source", "results"), 1);
    let first = runtime.goals_after(0, 1).unwrap().remove(0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..4 {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert_eq!(runtime.goals_after(0, 1).unwrap()[0].before, first.before);
    assert_eq!(count(&f, "source", "attempts"), 2);
    host.shutdown().unwrap();
}
#[test]
fn malformed_request_data_fails_only_its_own_goal() {
    let f = fixture();
    peer(&f, "bad", "invalid");
    peer(&f, "good", "ready");
    routes(&f, &["bad", "good"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        let r = rt.report(0).unwrap();
        r.failed_jobs == 1 && r.awaiting_approval == 1
    });
    assert!(host.request_workers().is_empty());
    let bad = runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.route_id == "bad")
        .unwrap();
    assert_eq!(bad.state, GoalState::Failed);
    assert!(bad.before.is_empty());
    assert!(bad.candidate.is_none());
    assert!(bad.action.is_none());
    for _ in 0..4 {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert_eq!(count(&f, "bad", "attempts"), 1);
    host.shutdown().unwrap();
}
#[test]
fn paused_queries_drain_without_advancing_and_resume_requires_fresh_sources() {
    let f = fixture();
    peer(&f, "source", "gate");
    routes(&f, &["source"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| started(&f, "source"));
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    let command = |revision, id: &str, change| TaskControlCommand {
        sequence: task.sequence,
        expected_revision: revision,
        command_id: id.into(),
        operator: Actor {
            actor_type: ActorType::Human,
            id: "request-test".into(),
        },
        reason: "test source lifecycle".into(),
        change,
    };
    runtime
        .control_task(&mut host, command(0, "pause", TaskChange::Pause))
        .unwrap();
    release(&f, "source");
    until(&mut host, &mut runtime, |h, _| {
        h.request_workers().is_empty()
    });
    let paused = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(paused.state, GoalState::Acquiring);
    assert!(paused.before.is_empty());
    assert!(paused.candidate.is_none());
    assert_eq!(count(&f, "source", "attempts"), 1);
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
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    assert_eq!(count(&f, "source", "attempts"), 2);
    assert_eq!(count(&f, "source", "results"), 2);
    assert_eq!(runtime.goals_after(0, 1).unwrap()[0].round, 2);
    host.shutdown().unwrap();
}
#[test]
fn native_async_queries_preserve_manual_approvals_and_confirmed_effects() {
    let f = fixture();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["minimum_actuations"] = json!(1);
    input["events"][0]["payload"]["enabled"] = json!(true);
    f.write("input-profile.json", input);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
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
            task.sequence,
            task.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "request-test".into(),
            },
            "explicit simulator test review".into(),
        )
        .unwrap();
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().completed_goals == 1
    });
    assert!(host.request_workers().is_empty());
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    for _ in 0..4 {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert_eq!(
        f.db("fan")
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    host.shutdown().unwrap();
}

#[test]
fn independent_sources_of_one_goal_start_before_either_reply_is_released() {
    let f = fixture();
    peer(&f, "a", "gate");
    peer(&f, "b", "gate");
    let mut config = f.read("runtime.json");
    let first = config["routes"][0]["goal"]["sources"][0].clone();
    config["routes"][0]["goal"]["sources"] = json!([first.clone(), first]);
    for (i, id) in ["a", "b"].iter().enumerate() {
        config["routes"][0]["goal"]["sources"][i]["id"] =
            json!(if i == 0 { "fan-state" } else { "second-state" });
        config["routes"][0]["goal"]["sources"][i]["capability"]["adapter_id"] = json!(id);
    }
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| {
        started(&f, "a") && started(&f, "b")
    });
    assert_eq!(host.request_workers().len(), 2);
    release(&f, "a");
    release(&f, "b");
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(task.before.len(), 2);
    assert_ne!(task.before[0], task.before[1]);
    let a = host.store().get(&task.before[0]).unwrap().unwrap();
    let b = host.store().get(&task.before[1]).unwrap().unwrap();
    assert_eq!(a.created_by.id, "a");
    assert_eq!(b.created_by.id, "b");
    host.shutdown().unwrap();
}
#[test]
fn committed_cancellation_discards_a_late_source_without_blocking_other_goals() {
    let f = fixture();
    peer(&f, "slow", "gate");
    peer(&f, "fast", "ready");
    routes(&f, &["slow", "fast"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        started(&f, "slow") && rt.report(0).unwrap().awaiting_approval == 1
    });
    let task = runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.route_id == "slow")
        .unwrap();
    f.db("runtime").execute_batch("CREATE TRIGGER reject_cancel BEFORE UPDATE OF state ON runtime_jobs WHEN NEW.state='cancelled' BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    assert!(
        runtime
            .cancel_goal(&host, task.sequence, "test cancellation".into())
            .is_err()
    );
    runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    assert!(!host.request_workers()[0].discarded);
    f.db("runtime")
        .execute_batch("DROP TRIGGER reject_cancel")
        .unwrap();
    runtime
        .cancel_goal(&host, task.sequence, "committed test cancellation".into())
        .unwrap();
    runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    assert!(host.request_workers()[0].discarded);
    release(&f, "slow");
    until(&mut host, &mut runtime, |h, _| {
        h.request_workers().is_empty()
    });
    let task = runtime
        .goals_after(0, 10)
        .unwrap()
        .into_iter()
        .find(|g| g.route_id == "slow")
        .unwrap();
    assert_eq!(task.state, GoalState::Cancelled);
    assert!(task.before.is_empty());
    assert!(task.candidate.is_none());
    let results:i64=f.db("core").query_row("SELECT COUNT(*) FROM artifact_versions WHERE schema_id='arcs.state.fan.v1' AND json_extract(artifact_json,'$.created_by.id')='slow'",[],|r|r.get(0)).unwrap();
    assert_eq!(results, 0);
    assert_eq!(runtime.report(0).unwrap().awaiting_approval, 1);
    host.shutdown().unwrap();
}
#[test]
fn a_timed_out_source_does_not_stop_a_healthy_goal_or_restart_its_query() {
    let f = fixture();
    peer(&f, "slow", "gate");
    peer(&f, "fast", "ready");
    routes(&f, &["slow", "fast"]);
    let mut config = f.read("host.json");
    for adapter in config["adapters"].as_array_mut().unwrap() {
        if adapter["grant_file"] == "slow-grant.json" {
            adapter["timeout_ms"] = json!(1000);
        }
    }
    f.write("host.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        let r = rt.report(0).unwrap();
        r.failed_jobs == 1 && r.awaiting_approval == 1
    });
    assert!(host.request_workers().is_empty());
    assert_eq!(count(&f, "slow", "attempts"), 1);
    for _ in 0..4 {
        runtime.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert_eq!(count(&f, "slow", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn two_runtimes_share_the_same_request_worker_bound() {
    use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
    let f = fixture();
    for id in ["a", "b", "c"] {
        peer(&f, id, "gate");
    }
    routes(&f, &["a", "b"]);
    let primary = f.read("runtime.json");
    routes(&f, &["c"]);
    let mut secondary = f.read("runtime.json");
    secondary["runtime_id"] = json!("second-runtime");
    secondary["journal"] = json!(f.directory.join("second-runtime.sqlite"));
    f.write("second-runtime.json", secondary);
    f.write("runtime.json", primary);
    let mut host = f.host();
    let mut first = f.runtime(&host);
    let mut second = TaskRuntime::open(
        &host,
        RuntimeConfig::load(f.directory.join("second-runtime.json")).unwrap(),
    )
    .unwrap();
    until(&mut host, &mut first, |_, _| {
        started(&f, "a") && started(&f, "b")
    });
    for _ in 0..5 {
        second.tick_at(&mut host, 1_800_000_000_000).unwrap();
    }
    assert_eq!(host.request_workers().len(), 2);
    assert!(!started(&f, "c"));
    release(&f, "a");
    until(&mut host, &mut first, |h, _| h.request_workers().len() == 1);
    until(&mut host, &mut second, |_, _| started(&f, "c"));
    assert_eq!(host.request_workers().len(), 2);
    release(&f, "b");
    release(&f, "c");
    until(&mut host, &mut first, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 2
    });
    until(&mut host, &mut second, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    host.shutdown().unwrap();
}

#[test]
fn an_expired_goal_cannot_adopt_a_late_query_response() {
    let f = fixture();
    peer(&f, "source", "gate");
    routes(&f, &["source"]);
    let mut config = f.read("runtime.json");
    config["routes"][0]["max_lifetime_ms"] = json!(100);
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| started(&f, "source"));
    let expired = runtime.tick_at(&mut host, 1_800_000_000_101).unwrap();
    assert_eq!(expired.expired_tasks.len(), 1);
    assert!(host.request_workers()[0].discarded);
    release(&f, "source");
    until(&mut host, &mut runtime, |h, _| {
        h.request_workers().is_empty()
    });
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(task.state, GoalState::Cancelled);
    assert!(task.before.is_empty());
    assert!(task.candidate.is_none());
    assert_eq!(runtime.report(0).unwrap().expired_jobs, 1);
    host.shutdown().unwrap();
}
#[test]
fn invalid_request_concurrency_is_rejected_without_creating_runtime_work() {
    use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
    let f = fixture();
    let mut host = f.host();
    for limit in [0, 33] {
        let mut config = f.read("runtime.json");
        config["max_concurrent_requests"] = json!(limit);
        f.write("runtime.json", config);
        assert!(
            TaskRuntime::open(
                &host,
                RuntimeConfig::load(f.directory.join("runtime.json")).unwrap()
            )
            .is_err()
        );
    }
    assert!(host.request_workers().is_empty());
    assert!(!f.directory.join("runtime.sqlite").exists());
    host.shutdown().unwrap();
}

#[test]
fn a_recorded_source_failure_repairs_a_lost_task_checkpoint_without_resending() {
    let f = fixture();
    peer(&f, "source", "failed_gate");
    routes(&f, &["source"]);
    use_default_limits(&f);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| started(&f, "source"));
    let request = host.request_workers().remove(0);
    let journal = f.db("runtime");
    journal
        .execute_batch(
            "CREATE TRIGGER reject_query_failure BEFORE UPDATE OF state ON runtime_jobs
        WHEN NEW.state='failed' BEGIN SELECT RAISE(ABORT,'injected task failure checkpoint'); END;",
        )
        .unwrap();
    release(&f, "source");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let error = loop {
        match runtime.tick_at(&mut host, 1_800_000_000_000) {
            Err(error) => break error,
            Ok(_) => {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    };
    assert!(
        error
            .to_string()
            .contains("injected task failure checkpoint"),
        "{error}"
    );
    let saved = arcs::runtime::InvocationService::new(
        host.store(),
        host.schemas(),
        &arcs::core::SystemClock,
    )
    .lookup(&request.invocation_id)
    .unwrap()
    .unwrap();
    assert_eq!(saved.status, arcs::runtime::InvocationStatus::Failed);
    assert!(saved.retryable);
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(task.state, GoalState::Acquiring);
    journal
        .execute_batch("DROP TRIGGER reject_query_failure")
        .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().failed_jobs == 1
    });
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(task.state, GoalState::Failed);
    assert!(task.detail.contains("RecordedFailure"));
    assert!(task.detail.contains("simulated source failure"));
    assert!(task.before.is_empty());
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert_eq!(count(&f, "source", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_core_result_commit_failure_is_propagated_and_does_not_publish_partial_source_data() {
    use arcs_host::HostError;
    use std::time::{Duration, Instant};
    let f = fixture();
    peer(&f, "source", "gate");
    routes(&f, &["source"]);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, _| started(&f, "source"));
    f.db("core").execute_batch("CREATE TRIGGER reject_source BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.state.fan.v1' AND json_extract(NEW.artifact_json,'$.created_by.id')='source' BEGIN SELECT RAISE(ABORT,'injected Core commit failure'); END;").unwrap();
    release(&f, "source");
    let deadline = Instant::now() + Duration::from_secs(8);
    let error = loop {
        match runtime.tick_at(&mut host, 1_800_000_000_000) {
            Err(error) => break error,
            Ok(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert!(
        matches!(
            error,
            HostError::Request {
                source: arcs::request::RequestError::Invocation(
                    arcs::runtime::InvocationError::Store(_)
                ),
                ..
            }
        ),
        "unexpected error: {error:?}"
    );
    let invocation_id = std::fs::read_to_string(f.directory.join("source.started")).unwrap();
    let saved = arcs::runtime::InvocationService::new(
        host.store(),
        host.schemas(),
        &arcs::core::SystemClock,
    )
    .lookup(&invocation_id)
    .unwrap()
    .unwrap();
    assert_eq!(saved.status, arcs::runtime::InvocationStatus::Dispatched);
    assert!(saved.result_version.is_none());
    let results:i64=f.db("core").query_row("SELECT COUNT(*) FROM artifact_versions WHERE schema_id='arcs.state.fan.v1' AND json_extract(artifact_json,'$.created_by.id')='source'",[],|r|r.get(0)).unwrap();
    assert_eq!(results, 0);
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert!(task.before.is_empty());
    assert!(task.candidate.is_none());
    assert!(task.action.is_none());
    assert_eq!(count(&f, "source", "attempts"), 1);
    f.db("core")
        .execute_batch("DROP TRIGGER reject_source")
        .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    until(&mut host, &mut runtime, |_, rt| {
        rt.report(0).unwrap().awaiting_approval == 1
    });
    let saved = arcs::runtime::InvocationService::new(
        host.store(),
        host.schemas(),
        &arcs::core::SystemClock,
    )
    .lookup(&invocation_id)
    .unwrap()
    .unwrap();
    assert_eq!(saved.status, arcs::runtime::InvocationStatus::Succeeded);
    let task = runtime.goals_after(0, 1).unwrap().remove(0);
    assert_eq!(saved.result_version.as_ref(), task.before.first());
    assert_eq!(count(&f, "source", "attempts"), 2);
    assert_eq!(count(&f, "source", "results"), 1);
    assert_eq!(
        std::fs::read_to_string(f.directory.join("source.started")).unwrap(),
        invocation_id
    );
    host.shutdown().unwrap();
}

#[path = "support/goals.rs"]
mod goals;
#[path = "support/parallel.rs"]
mod parallel;
mod support;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Monitor {
    child: Child,
    stop_file: PathBuf,
    events: Receiver<Value>,
}

impl Monitor {
    fn start(directory: &Path) -> Self {
        let stop_file = directory.join("monitor.stop");
        let mut child = Command::new(env!("CARGO_BIN_EXE_arcs-host"))
            .arg("--config")
            .arg(directory.join("host.json"))
            .arg("--runtime")
            .arg(directory.join("runtime.json"))
            .args(["--monitor", "--control-stdin", "--stop-file"])
            .arg(&stop_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, events) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let event = serde_json::from_str(&line.unwrap()).unwrap();
                if sender.send(event).is_err() {
                    break;
                }
            }
        });
        let monitor = Self {
            child,
            stop_file,
            events,
        };
        let ready = monitor.until(|event| event["event"] == "ready");
        assert_eq!(ready["control"]["transport"], "stdin_jsonl");
        assert_eq!(ready["control"]["resource_release_queue"], true);
        monitor
    }

    fn until(&self, condition: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let budget = deadline
                .checked_duration_since(Instant::now())
                .expect("monitor event deadline elapsed");
            let event = self.events.recv_timeout(budget).unwrap();
            if condition(&event) {
                return event;
            }
            assert_ne!(event["event"], "error", "{event}");
        }
    }

    fn command(&mut self, id: &str, kind: &str, command: Value) -> Value {
        let request = envelope(id, kind, command);
        writeln!(self.child.stdin.as_mut().unwrap(), "{request}").unwrap();
        self.until(|event| event["event"] == "control_result" && event["request_id"] == id)
    }

    fn stop(mut self) {
        std::fs::write(&self.stop_file, "stop").unwrap();
        self.until(|event| event["event"] == "stopped");
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn envelope(id: &str, kind: &str, command: Value) -> Value {
    json!({"request_id":id,"control":{"kind":kind,"command":command}})
}

fn task_control(revision: u64, change: &str) -> Value {
    json!({"sequence":2,"expected_revision":revision,"command_id":format!("task-{change}"),
        "operator":{"actor_type":"human","id":"operator.test"},"reason":"monitor task control",
        "change":{"command":change}})
}

fn series_control(revision: u64, change: &str) -> Value {
    json!({"sequence":1,"expected_revision":revision,"command_id":format!("series-{change}"),
        "operator":{"actor_type":"human","id":"operator.test"},"reason":"monitor series control",
        "change":change})
}

#[test]
fn queue_pages_keep_their_sequence_bound_and_show_live_waiting_and_pause_states() {
    let f = parallel::Fixture::new();
    f.peer("a", "normal", false);
    for route in ["scheduled", "immediate", "dependent", "signal"] {
        f.route(route, "a");
    }
    let mut config = f.read("runtime.json");
    config["interval_ms"] = json!(15);
    config["routes"][0]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    config["routes"][0]["priority"] = json!(3);
    config["routes"][0]["max_lifetime_ms"] = json!(2_592_000_000u64);
    config["routes"][2]["dependencies"] =
        json!([{"route_id":"scheduled","payload_fields":["value"]}]);
    config["routes"][2]["budget"]["max_context_items"] = json!(2);
    config["routes"][3]["activation"] = json!({"kind":"on_event","source":{"adapter_id":"example.input","capability_id":"task.submit"},
        "schema_id":"arcs.task.nearest.v1","subject":"later-signal-only"});
    f.write("runtime.json", config);
    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    let first = monitor.command("page-1", "queue", json!({"limit":1}));
    assert_eq!(first["ok"], true, "{first}");
    let first = &first["receipt"];
    let task = &first["tasks"][0];
    assert_eq!(first["through_sequence"], 4);
    assert_eq!(first["next_after_sequence"], 1);
    assert_eq!(task["sequence"], 1);
    assert_eq!(task["waiting_for"], "time");
    assert_eq!(task["not_before_unix_ms"], i64::MAX);
    assert!(task["deadline_unix_ms"].as_u64().is_some());
    assert_eq!(task["priority"], 3);
    assert_eq!(task["control_revision"], 0);
    assert_eq!(task["can_control"], true);
    assert_eq!(
        monitor.command("huge-page", "queue", json!({"limit":101}))["ok"],
        false
    );
    assert_eq!(
        monitor.command(
            "bad-range",
            "queue",
            json!({"limit":1,"after_sequence":5,"through_sequence":4})
        )["ok"],
        false
    );
    assert_eq!(
        monitor.command(
            "future-range",
            "queue",
            json!({"limit":1,"through_sequence":5})
        )["ok"],
        false
    );

    let mut pause = task_control(0, "pause");
    pause["sequence"] = json!(1);
    assert_eq!(monitor.command("pause-pending", "task", pause)["ok"], true);
    let changed = monitor.command(
        "verify-pause",
        "queue",
        json!({"limit":1,"through_sequence":first["through_sequence"]}),
    );
    assert_eq!(changed["receipt"]["tasks"][0]["locally_paused"], true);
    assert_eq!(changed["receipt"]["tasks"][0]["paused"], true);
    assert_eq!(changed["receipt"]["tasks"][0]["control_revision"], 1);

    f.write(
        "new-input.json",
        json!({"id":"queue-new","subject":"queue-new","reference":"task:queue-new",
        "payload":{"values":[2,4],"target":3}}),
    );
    let queued = Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(f.directory.join("input-profile.json"))
        .arg(f.directory.join("input.sqlite"))
        .arg(f.directory.join("new-input.json"))
        .output()
        .unwrap();
    assert!(
        queued.status.success(),
        "{}",
        String::from_utf8_lossy(&queued.stderr)
    );
    monitor.until(|event| event["runtime"]["ready_for_review"] == 2);
    let second = monitor.command(
        "page-2",
        "queue",
        json!({"limit":10,"after_sequence":first["next_after_sequence"],
        "through_sequence":first["through_sequence"]}),
    );
    let page = &second["receipt"];
    assert_eq!(page["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(page["tasks"][0]["sequence"], 3);
    assert_eq!(page["tasks"][0]["waiting_for"], "dependency");
    assert_eq!(page["tasks"][1]["waiting_for"], "event");
    assert!(page["next_after_sequence"].is_null());

    let all = monitor.command("history", "queue", json!({"limit":10,"scope":"all"}));
    assert_eq!(all["receipt"]["through_sequence"], 8);
    assert_eq!(all["receipt"]["tasks"].as_array().unwrap().len(), 8);
    assert_eq!(all["receipt"]["tasks"][1]["state"], "ready_for_review");
    assert_eq!(all["receipt"]["tasks"][1]["can_control"], false);
    assert!(all["receipt"]["tasks"][1].get("payload").is_none());
    let mut cancel = task_control(0, "cancel");
    cancel["sequence"] = json!(1);
    let stale = monitor.command("stale-cancel", "task", cancel.clone());
    assert_eq!(stale["ok"], false);
    assert!(
        stale["error"]["message"]
            .as_str()
            .unwrap()
            .contains("revision conflict")
    );
    cancel["expected_revision"] = json!(1);
    let stopped = monitor.command("cancel-pending", "task", cancel.clone());
    assert_eq!(stopped["receipt"]["revision"], 2);
    assert_eq!(stopped["receipt"]["cancelled"], true);
    assert_eq!(
        monitor.command("cancel-replay", "task", cancel)["receipt"],
        stopped["receipt"]
    );
    let mut resume = task_control(2, "resume");
    resume["sequence"] = json!(1);
    assert_eq!(monitor.command("resume-ended", "task", resume)["ok"], false);
    let all = monitor.command(
        "cancelled-history",
        "queue",
        json!({"limit":1,"scope":"all"}),
    );
    assert_eq!(all["receipt"]["tasks"][0]["state"], "cancelled");
    assert_eq!(all["receipt"]["tasks"][0]["can_control"], false);
    assert_eq!(f.count("a", "attempts"), 2);
    monitor.stop();
}

#[test]
fn live_series_and_task_control_survive_bad_frames_eof_and_restart() {
    let f = parallel::Fixture::new();
    f.peer("a", "normal", false);
    f.route("recurring", "a");
    f.route("scheduled", "a");
    let mut config = f.read("runtime.json");
    config["interval_ms"] = json!(15);
    config["routes"][0]["recurrence"] = json!({"every_ms":2592000000u64,"max_occurrences":4});
    config["routes"][1]["activation"] = json!({"kind":"at_time","unix_ms":i64::MAX});
    f.write("runtime.json", config);
    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    let pause = monitor.command("pause-1", "series", series_control(0, "pause"));
    assert_eq!(pause["receipt"]["state"], "paused");
    assert_eq!(
        monitor.command("pause-replay", "series", series_control(0, "pause"))["receipt"],
        pause["receipt"]
    );
    assert_eq!(
        monitor.command("pause-task", "task", task_control(0, "pause"))["receipt"]["paused"],
        true
    );

    let unsupported = monitor.command("bad-kind", "adapter_call", json!({}));
    assert_eq!(unsupported["ok"], false);
    assert_eq!(unsupported["error"]["code"], "invalid_request");
    let stale = monitor.command("resume-stale", "series", series_control(0, "resume"));
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "operation_failed");
    let input = monitor.child.stdin.as_mut().unwrap();
    input.write_all(b"{bad json}\n").unwrap();
    input.write_all(&vec![b'x'; 70_000]).unwrap();
    input.write_all(b"\n").unwrap();
    let malformed = monitor.until(|event| event["event"] == "control_result");
    assert!(malformed["request_id"].is_null());
    assert_eq!(malformed["error"]["code"], "invalid_request");
    let oversized = monitor.until(|event| event["event"] == "control_result");
    assert_eq!(oversized["error"]["code"], "invalid_frame");
    assert!(
        oversized["error"]["message"]
            .as_str()
            .unwrap()
            .contains("65536")
    );

    let input = monitor.child.stdin.as_mut().unwrap();
    for index in 0..20 {
        let replay = envelope(
            &format!("burst-{index}"),
            "series",
            series_control(0, "pause"),
        );
        writeln!(input, "{replay}").unwrap();
    }
    let mut since_snapshot = 0;
    for index in 0..20 {
        let expected_id = format!("burst-{index}");
        loop {
            let event = monitor.until(|event| {
                matches!(event["event"].as_str(), Some("snapshot" | "control_result"))
            });
            if event["event"] == "snapshot" {
                since_snapshot = 0;
                continue;
            }
            assert_eq!(event["request_id"], expected_id);
            assert_eq!(event["receipt"], pause["receipt"]);
            since_snapshot += 1;
            assert!(
                since_snapshot <= 8,
                "control backlog must yield to the runtime tick"
            );
            break;
        }
    }

    assert_eq!(
        monitor.command("resume", "series", series_control(1, "resume"))["receipt"]["state"],
        "active"
    );
    assert_eq!(
        monitor.command("cancel", "series", series_control(2, "cancel"))["receipt"]["state"],
        "cancelled"
    );
    // Even valid JSON requires its framing newline. EOF must not apply this resume.
    write!(
        monitor.child.stdin.as_mut().unwrap(),
        "{}",
        envelope("incomplete", "task", task_control(1, "resume"))
    )
    .unwrap();
    drop(monitor.child.stdin.take());
    let incomplete = monitor.until(|event| event["event"] == "control_result");
    assert_eq!(incomplete["error"]["code"], "invalid_frame");
    let snapshot = monitor.until(|event| event["event"] == "snapshot");
    assert_eq!(snapshot["runtime"]["paused_jobs"], 1);
    assert_eq!(
        snapshot["runtime"]["recurring_checks"][0]["state"],
        "cancelled"
    );
    monitor.stop();

    let (mut host, mut runtime) = f.open();
    runtime.tick(&mut host).unwrap();
    let report = runtime.report(0).unwrap();
    assert_eq!(report.paused_jobs, 1);
    assert_eq!(report.recurring_checks[0].control_revision, 3);
    assert_eq!(report.ready_for_review, 1);
    assert_eq!(f.count("a", "attempts"), 1);
    host.shutdown().unwrap();
}

#[test]
fn live_goal_review_requires_the_current_candidate_and_executes_once() {
    let f = goals::Fixture::new();
    f.manual();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", input);
    let mut monitor = Monitor::start(&f.directory);
    let waiting = monitor.until(|event| event["runtime"]["awaiting_approval"] == 1);
    let task = &waiting["runtime"]["goals"][0];
    let fan_count = || -> u64 {
        let db = rusqlite::Connection::open_with_flags(
            f.directory.join("fan.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get::<_, i64>(0).map(|count| count as u64)
        })
        .unwrap()
    };
    let approval = |candidate: &Value| {
        json!({"command":"review","sequence":task["sequence"],
        "candidate_version_id":candidate,"decision":"approved","operator_id":"simulator.test",
        "reason":"test approval for the local fan simulator"})
    };
    assert_eq!(fan_count(), 0);
    let inspect = |candidate: &Value| json!({"command":"inspect","sequence":task["sequence"],"candidate_version_id":candidate});
    assert_eq!(
        monitor.command("old-preview", "goal", inspect(&json!("wrong-candidate")))["ok"],
        false
    );
    let preview = monitor.command("preview", "goal", inspect(&task["candidate"]));
    assert_eq!(preview["ok"], true);
    assert_eq!(
        preview["receipt"]["candidate_version_id"],
        task["candidate"]
    );
    assert_eq!(
        preview["receipt"]["verification_version_id"],
        task["verification"]
    );
    assert_eq!(preview["receipt"]["capability"], task["capability"]);
    assert_eq!(
        preview["receipt"]["payload"],
        json!({"device_id":"fan-01","enabled":true})
    );
    assert_eq!(fan_count(), 0);
    let stale = monitor.command(
        "stale-candidate",
        "goal",
        approval(&json!("wrong-candidate")),
    );
    assert_eq!(stale["ok"], false);
    assert_eq!(fan_count(), 0);
    let accepted = monitor.command("approve", "goal", approval(&task["candidate"]));
    assert_eq!(accepted["ok"], true);
    monitor.until(|event| event["runtime"]["completed_goals"] == 1);
    assert_eq!(
        monitor.command("approve-again", "goal", approval(&task["candidate"]))["ok"],
        false
    );
    monitor.stop();
    assert_eq!(fan_count(), 1);

    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    runtime.tick(&mut host).unwrap();
    assert_eq!(runtime.report(0).unwrap().completed_goals, 1);
    assert_eq!(fan_count(), 1);
    host.shutdown().unwrap();
}

#[test]
fn a_live_controller_release_keeps_independent_work_running_and_resumes_after_restart() {
    use arcs::approval::ApprovalDecision;
    use arcs::core::{Actor, ActorType};
    use arcs_host::task_runtime::goal::GoalState;

    let f = goals::Fixture::new();
    f.manual();
    let background = add_independent_route(&f);
    gate_fan(&f, "close_fence");
    let mut fan = f.read("fan-profile.json");
    fan["fenced_resource"] = json!("device.fan-01");
    f.write("fan-profile.json", fan);
    let mut config = f.read("runtime.json");
    config["interval_ms"] = json!(10);
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device.fan-01", "workcell.fan"]);
    config["routes"][0]["goal"]["actions"][0]["fenced_resource"] = json!("device.fan-01");
    f.write("runtime.json", config);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let until =
        |host: &mut arcs_host::Host, runtime: &mut arcs_host::task_runtime::TaskRuntime, state| {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                runtime.tick(host).unwrap();
                if let Some(goal) = runtime.goals_after(0, 1).unwrap().into_iter().next()
                    && goal.state == state
                {
                    return goal;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(3));
            }
        };
    let goal = until(&mut host, &mut runtime, GoalState::AwaitingApproval);
    let actor = Actor {
        actor_type: ActorType::Human,
        id: "simulator.test".into(),
    };
    runtime
        .review_goal(
            &host,
            goal.sequence,
            goal.candidate.as_ref().unwrap(),
            ApprovalDecision::Approved,
            actor,
            "simulated action".into(),
        )
        .unwrap();
    let goal = until(&mut host, &mut runtime, GoalState::Observing);
    let evidence = goal.execution.unwrap();
    runtime
        .cancel_goal(&host, goal.sequence, "settle the simulated dispatch".into())
        .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let mut monitor = Monitor::start(&f.directory);
    let snapshot = monitor.until(|event| event["event"] == "snapshot");
    let review = &snapshot["resource_release_reviews"][0];
    assert_eq!(review["sequence"], goal.sequence);
    assert_eq!(review["state"], "cancelled");
    assert_eq!(review["execution"], evidence.0);
    let reservations = review["reservations"].as_array().unwrap();
    assert_eq!(reservations.len(), 2);
    let versions = reservations
        .iter()
        .map(|record| record["version_id"].clone())
        .collect::<Vec<_>>();
    let inspect = |versions: &[Value], evidence_versions: Value| {
        json!({
            "command":"inspect_resource_evidence", "sequence":goal.sequence,
            "reservation_versions":versions, "evidence_versions":evidence_versions
        })
    };
    let refs = json!([&evidence]);
    assert_eq!(
        monitor.command("partial", "goal", inspect(&versions[..1], refs.clone()))["ok"],
        false
    );
    assert_eq!(
        monitor.command(
            "missing",
            "goal",
            inspect(&versions, json!(["missing-version"]))
        )["ok"],
        false
    );
    assert_eq!(
        monitor.command(
            "too-many",
            "goal",
            inspect(&versions, json!(vec![&evidence; 33]))
        )["ok"],
        false
    );
    let preview = monitor.command("preview", "goal", inspect(&versions, refs.clone()));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["receipt"]["reservation_versions"], json!(versions));
    assert_eq!(preview["receipt"]["artifacts"][0]["version_id"], evidence.0);
    assert!(preview["receipt"]["artifacts"][0]["payload"].is_object());
    let release = json!({"command":"release_resources","sequence":goal.sequence,
        "reservation_versions":versions,"evidence_versions":refs,"operator_id":"simulator.test",
        "reason":"simulated state was checked"});
    let accepted = monitor.command("release", "goal", release.clone());
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert_eq!(accepted["receipt"]["release_pending"], true);
    monitor.until(|event| {
        f.directory.join("close.entered").exists()
            && event["controller_closures"]
                .as_array()
                .is_some_and(|rows| rows.len() == 1)
    });
    assert_eq!(
        monitor.command("retry-pending", "goal", release.clone())["receipt"]["release_pending"],
        true
    );
    let mut changed = release;
    changed["reason"] = json!("another decision");
    assert_eq!(
        monitor.command("changed-pending", "goal", changed)["ok"],
        false
    );

    enqueue_independent_work(&background, "background");
    let running = monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    assert_eq!(running["resource_reservations"][0]["state"], "held");
    assert_eq!(
        running["resource_release_reviews"][0]["release_requested"],
        true
    );
    assert_eq!(running["controller_closures"].as_array().unwrap().len(), 1);
    monitor.stop();

    // The operator decision survives shutdown while the old transport is blocked.
    std::fs::remove_file(f.directory.join("monitor.stop")).unwrap();
    std::fs::remove_file(f.directory.join("close.entered")).unwrap();
    let mut monitor = Monitor::start(&f.directory);
    monitor.until(|event| {
        f.directory.join("close.entered").exists()
            && event["resource_release_reviews"][0]["release_requested"] == true
    });
    std::fs::write(f.directory.join("close.release"), "continue").unwrap();
    let snapshot = monitor.until(|event| event["resource_reservations"][0]["state"] == "released");
    assert!(
        snapshot["resource_release_reviews"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        snapshot["resource_reservations"][1]["evidence"],
        json!([&evidence])
    );
    assert_eq!(
        snapshot["resource_reservations"][0]["decision_by"]["id"],
        "simulator.test"
    );
    let actuations: i64 = f
        .db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(actuations, 1);
    assert_eq!(
        monitor.command("old-preview", "goal", inspect(&versions, json!([evidence])))["ok"],
        false
    );
    monitor.stop();
}

#[test]
fn an_act_reply_can_be_delayed_across_restart_while_independent_work_completes() {
    let f = goals::Fixture::new();
    let background = add_independent_route(&f);
    let (monitor, action) = dispatch_with_held_reply(&f, "execute_fenced");
    enqueue_independent_work(&background, "background");
    let running = monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    assert_eq!(running["execution_workers"][0], action);
    assert_eq!(running["runtime"]["goals"][0]["state"], "executing");
    assert!(running["runtime"]["goals"][0]["execution"].is_null());
    assert_eq!(running["resource_reservations"][0]["state"], "held");
    monitor.stop();

    std::fs::remove_file(f.directory.join("monitor.stop")).unwrap();
    std::fs::remove_file(f.directory.join("act.entered")).unwrap();
    let monitor = Monitor::start(&f.directory);
    let replay = monitor.until(|event| {
        f.directory.join("act.entered").exists() && event["execution_workers"][0] == action
    });
    assert_eq!(replay["runtime"]["ready_for_review"], 1);
    assert_eq!(
        std::fs::read_to_string(f.directory.join("act.entered")).unwrap(),
        action["invocation_id"].as_str().unwrap()
    );
    std::fs::write(f.directory.join("act.release"), "continue").unwrap();
    let snapshot = monitor.until(|event| {
        matches!(
            event["runtime"]["goals"][0]["state"].as_str(),
            Some("completed" | "awaiting_approval" | "blocked" | "failed")
        )
    });
    assert_eq!(
        snapshot["runtime"]["completed_goals"], 1,
        "{}",
        snapshot["runtime"]["goals"]
    );
    assert!(snapshot["execution_workers"].as_array().unwrap().is_empty());
    let goal = &snapshot["runtime"]["goals"][0];
    assert_eq!(goal["action"], action["action_version"]);
    let recorded_at: i64 = f
        .db("core")
        .query_row(
            "SELECT sequence FROM artifact_versions WHERE version_id=?1",
            [goal["execution"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(goal["observation_cursor"], recorded_at);
    assert!(recorded_at > goal["dispatch_cursor"].as_i64().unwrap());
    assert_fan_dispatched_once(&f, &action);
    monitor.stop();
}

#[test]
fn a_cancelled_goal_keeps_its_reservation_and_audits_the_delayed_act_reply() {
    let f = goals::Fixture::new();
    let (mut monitor, action) = dispatch_with_held_reply(&f, "execute_fenced");
    let cancelled = monitor.command(
        "cancel-during-act",
        "goal",
        json!({
            "command":"cancel","sequence":action["sequence"],"reason":"stop the simulated task"
        }),
    );
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let still_running = monitor.until(|event| event["runtime"]["goals"][0]["state"] == "cancelled");
    assert_eq!(still_running["execution_workers"][0], action);
    std::fs::write(f.directory.join("act.release"), "continue").unwrap();
    let settled = monitor.until(|event| {
        event["runtime"]["goals"][0]["state"] == "cancelled"
            && event["execution_workers"]
                .as_array()
                .is_some_and(Vec::is_empty)
    });
    assert_eq!(settled["resource_reservations"][0]["state"], "held");
    assert!(settled["runtime"]["goals"][0]["execution"].is_null());
    let evidence = &settled["resource_release_reviews"][0]["execution"];
    let id = action["invocation_id"].as_str().unwrap();
    let saved: String = f
        .db("core")
        .query_row(
            "SELECT version_id FROM artifact_versions WHERE stream_key=?1",
            [format!("execution:{id}")],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(evidence, &json!(saved));
    assert_fan_dispatched_once(&f, &action);
    monitor.stop();
}

#[test]
fn a_failed_act_retires_only_its_goal_in_the_default_scheduler() {
    let f = goals::Fixture::new();
    let background = add_independent_route(&f);
    let (mut monitor, action) = dispatch_with_held_reply(&f, "fail_execute_fenced");
    std::fs::write(f.directory.join("act.release"), "continue").unwrap();
    let failed = monitor.until(|event| event["runtime"]["failed_jobs"] == 1);
    assert_failed_act(&f, &action, &failed);
    let queue = monitor.command("failed-goal", "queue", json!({"limit":4,"scope":"all"}));
    assert_eq!(queue["receipt"]["tasks"][0]["state"], "failed");
    assert_eq!(queue["receipt"]["tasks"][0]["can_control"], false);

    enqueue_independent_work(&background, "after-failure");
    let continued = monitor.until(|event| event["runtime"]["ready_for_review"] == 1);
    assert_eq!(continued["runtime"]["failed_jobs"], 1);
    assert_eq!(continued["runtime"]["capacity_used"], 0);
    monitor.stop();

    std::fs::remove_file(f.directory.join("monitor.stop")).unwrap();
    std::fs::remove_file(f.directory.join("act.entered")).unwrap();
    let monitor = Monitor::start(&f.directory);
    enqueue_independent_work(&background, "after-restart");
    let restarted = monitor.until(|event| event["runtime"]["ready_for_review"] == 2);
    assert_failed_act(&f, &action, &restarted);
    assert!(!f.directory.join("act.entered").exists());
    monitor.stop();
}

#[test]
fn a_lost_task_failure_checkpoint_recovers_from_the_core_without_resending_act() {
    let f = goals::Fixture::new();
    let (mut monitor, action) = dispatch_with_held_reply(&f, "fail_execute_fenced");
    let journal = f.db("runtime");
    journal
        .execute_batch(
            "CREATE TRIGGER failed_goal_checkpoint BEFORE UPDATE OF state ON runtime_jobs
        WHEN NEW.state='failed' BEGIN SELECT RAISE(ABORT,'failed goal checkpoint'); END;",
        )
        .unwrap();
    std::fs::write(f.directory.join("act.release"), "continue").unwrap();
    let error = monitor.until(|event| event["event"] == "error");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("failed goal checkpoint"),
        "{error}"
    );
    assert!(!monitor.child.wait().unwrap().success());
    let states: (String, String) = journal
        .query_row(
            "SELECT state,json_extract(goal_json,'$.state') FROM runtime_jobs WHERE sequence=?1",
            [action["sequence"].as_i64().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(states, ("running".into(), "executing".into()));
    journal
        .execute_batch("DROP TRIGGER failed_goal_checkpoint")
        .unwrap();
    std::fs::remove_file(f.directory.join("act.entered")).unwrap();

    let monitor = Monitor::start(&f.directory);
    let reconciled = monitor.until(|event| event["runtime"]["failed_jobs"] == 1);
    assert_failed_act(&f, &action, &reconciled);
    assert!(
        reconciled["runtime"]["goals"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("RecordedFailure")
    );
    assert!(!f.directory.join("act.entered").exists());
    monitor.stop();
}

fn assert_failed_act(f: &goals::Fixture, action: &Value, snapshot: &Value) {
    let runtime = &snapshot["runtime"];
    assert_eq!(runtime["goals"][0]["state"], "failed");
    assert!(
        runtime["goals"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("simulated reply lost")
    );
    assert!(runtime["goals"][0]["execution"].is_null());
    assert_eq!(runtime["capacity_used"], 0);
    assert_eq!(runtime["pending_jobs"], 0);
    assert_eq!(snapshot["resource_reservations"][0]["state"], "held");
    assert_eq!(snapshot["resource_release_reviews"][0]["state"], "failed");
    let id = action["invocation_id"].as_str().unwrap();
    let core = arcs::store::SqliteArtifactStore::open(f.directory.join("core.sqlite")).unwrap();
    let schemas = arcs::core::SchemaRegistry::new();
    let state = arcs::runtime::InvocationService::new(&core, &schemas, &arcs::core::SystemClock)
        .lookup(id)
        .unwrap()
        .unwrap();
    assert_eq!(state.status, arcs::runtime::InvocationStatus::Failed);
    assert!(state.retryable);
    let actuations: i64 = f
        .db("fan")
        .query_row("SELECT actuation_count FROM fan_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(actuations, 1);
}

fn dispatch_with_held_reply(f: &goals::Fixture, response: &str) -> (Monitor, Value) {
    f.manual();
    gate_fan(f, response);
    let mut config = f.read("runtime.json");
    config["interval_ms"] = json!(10);
    config["routes"][0]["goal"]["exclusive_resources"] = json!(["device.fan-01"]);
    config["routes"][0]["goal"]["actions"][0]["fenced_resource"] = json!("device.fan-01");
    f.write("runtime.json", config);
    let mut fan = f.read("fan-profile.json");
    fan["fenced_resource"] = json!("device.fan-01");
    f.write("fan-profile.json", fan);
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"] =
        json!({"device_id":"fan-01","enabled":true,"minimum_actuations":1});
    f.write("input-profile.json", input);
    let mut monitor = Monitor::start(&f.directory);
    let waiting = monitor.until(|event| event["runtime"]["awaiting_approval"] == 1);
    let goal = &waiting["runtime"]["goals"][0];
    let approved = monitor.command("approve", "goal", json!({
        "command":"review","sequence":goal["sequence"],"candidate_version_id":goal["candidate"],
        "decision":"approved","operator_id":"simulator.test","reason":"approve the simulated action"
    }));
    assert_eq!(approved["ok"], true, "{approved}");
    let dispatched = monitor.until(|event| {
        f.directory.join("act.entered").exists() && event["execution_workers"][0].is_object()
    });
    let worker = dispatched["execution_workers"][0].clone();
    assert_eq!(
        std::fs::read_to_string(f.directory.join("act.entered")).unwrap(),
        worker["invocation_id"].as_str().unwrap()
    );
    assert!(dispatched["runtime"]["goals"][0]["dispatch_cursor"].is_u64());
    (monitor, worker)
}

fn assert_fan_dispatched_once(f: &goals::Fixture, action: &Value) {
    let id = action["invocation_id"].as_str().unwrap();
    let fan = f.db("fan");
    let (count, recorded_id): (i64, String) = fan
        .query_row(
            "SELECT actuation_count, (SELECT invocation_id FROM invocations) FROM fan_state",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(recorded_id, id);
    let committed: i64 = f
        .db("core")
        .query_row(
            "SELECT COUNT(*) FROM artifact_versions WHERE stream_key=?1",
            [format!("execution:{id}")],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(committed, 1);
}

fn enqueue_independent_work(background: &parallel::Fixture, id: &str) {
    background.write(
        "late.json",
        json!({"id":id,"subject":id,"reference":format!("task:{id}"),
        "payload":{"values":[2,4],"target":3}}),
    );
    let queued = Command::new(support::adapter_binary("arcs-json-input"))
        .arg("--enqueue")
        .arg(background.directory.join("input-profile.json"))
        .arg(background.directory.join("input.sqlite"))
        .arg(background.directory.join("late.json"))
        .output()
        .unwrap();
    assert!(
        queued.status.success(),
        "{}",
        String::from_utf8_lossy(&queued.stderr)
    );
}

fn gate_fan(f: &goals::Fixture, message: &str) {
    std::fs::write(
        f.directory.join("gated-fan.py"),
        r#"
import json, pathlib, subprocess, sys, time
root = pathlib.Path(__file__).parent
target = sys.argv[1]
peer = subprocess.Popen(sys.argv[2:], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def gate(name, value):
    (root / (name + ".entered")).write_text(value)
    while not (root / (name + ".release")).exists():
        time.sleep(.005)
try:
    for line in sys.stdin.buffer:
        message = json.loads(line)["message"]
        if target == message["type"] == "close_fence":
            gate("close", "entered")
        peer.stdin.write(line)
        peer.stdin.flush()
        reply = peer.stdout.readline()
        if not reply:
            break
        if message["type"] == "execute_fenced" and target in ("execute_fenced", "fail_execute_fenced"):
            gate("act", message["invocation"]["invocation_id"])
            if target == "fail_execute_fenced":
                envelope = json.loads(reply)
                envelope["message"] = {"type":"error", "code":"unavailable",
                                       "message":"simulated reply lost", "retryable":True}
                reply = (json.dumps(envelope) + "\n").encode()
        sys.stdout.buffer.write(reply)
        sys.stdout.buffer.flush()
finally:
    peer.stdin.close()
    peer.wait()
"#,
    )
    .unwrap();
    let mut host = f.read("host.json");
    host["adapters"][1] = json!({"program":"/usr/bin/python3", "grant_file":"fan-grant.json", "timeout_ms":15000,
        "args":[{"path":"gated-fan.py"},message,{"path":support::adapter_binary("arcs-simulated-fan")},
                {"path":"fan-profile.json"},{"path":"fan.sqlite"}]});
    f.write("host.json", host);
}

fn add_independent_route(f: &goals::Fixture) -> parallel::Fixture {
    let background = parallel::Fixture::new();
    background.peer("background", "normal", false);
    background.route("background", "background");
    let mut input = background.read("input-profile.json");
    input["events"] = json!([]);
    background.write("input-profile.json", input);
    let background_host = background.read("host.json");
    let mut host = f.read("host.json");
    host["schemas"].as_array_mut().unwrap().extend(
        background_host["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    for mut adapter in background_host["adapters"].as_array().unwrap().clone() {
        for field in ["profile", "grant_file"] {
            if let Some(relative) = adapter[field].as_str() {
                adapter[field] = json!(background.directory.join(relative));
            }
        }
        for arg in adapter["args"].as_array_mut().unwrap() {
            if let Some(relative) = arg["path"].as_str() {
                arg["path"] = json!(background.directory.join(relative));
            }
        }
        host["adapters"].as_array_mut().unwrap().push(adapter);
    }
    f.write("host.json", host);
    let mut runtime = f.read("runtime.json");
    runtime["routes"]
        .as_array_mut()
        .unwrap()
        .push(background.read("runtime.json")["routes"][0].clone());
    f.write("runtime.json", runtime);
    background
}

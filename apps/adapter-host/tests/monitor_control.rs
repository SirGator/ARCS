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
        monitor
    }

    fn until(&self, condition: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let budget = deadline
                .checked_duration_since(Instant::now())
                .expect("monitor event deadline elapsed");
            let event = self.events.recv_timeout(budget).unwrap();
            assert_ne!(event["event"], "error", "{event}");
            if condition(&event) {
                return event;
            }
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

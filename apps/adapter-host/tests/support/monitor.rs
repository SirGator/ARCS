use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub struct Monitor {
    pub child: Child,
    stop_file: PathBuf,
    events: Receiver<Value>,
}

impl Monitor {
    pub fn start(directory: &Path) -> Self {
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

    pub fn until(&self, condition: impl Fn(&Value) -> bool) -> Value {
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

    pub fn command(&mut self, id: &str, kind: &str, command: Value) -> Value {
        let request = envelope(id, kind, command);
        writeln!(self.child.stdin.as_mut().unwrap(), "{request}").unwrap();
        self.until(|event| event["event"] == "control_result" && event["request_id"] == id)
    }

    pub fn stop(mut self) {
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

pub fn envelope(id: &str, kind: &str, command: Value) -> Value {
    json!({"request_id":id,"control":{"kind":kind,"command":command}})
}

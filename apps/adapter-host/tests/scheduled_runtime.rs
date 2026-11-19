mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs_host::Host;
use arcs_host::task_runtime::scheduler::ScheduleState;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde_json::{Value, json};

const NOW: u64 = 1_800_000_000_000;
const MONTH: u64 = 30 * 24 * 60 * 60 * 1000;

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new(activation: Value) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "arcs-schedule-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let f = Self { directory };
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = root.join("apps/adapter-host/config/roles");
        for file in [
            "input-profile.json",
            "input-grant.json",
            "solver-profile.json",
            "solver-grant.json",
            "runtime.json",
        ] {
            std::fs::copy(source.join(file), f.directory.join(file)).unwrap();
        }
        let mut grant = f.read("solver-grant.json");
        grant["reasoning_limits_v2"]["resources"]["max_runtime_ms"] = json!(15000);
        f.write("solver-grant.json", grant);
        f.write("host.json", json!({"config_version":1,"database":"core.sqlite","schemas":[root.join("adapters/example-reasoner/schemas/task.schema.json"),root.join("adapters/example-reasoner/schemas/candidate.schema.json")],"adapters":[
            {"program":support::adapter_binary("arcs-json-input"),"profile":"input-profile.json","args":[{"path":"input.sqlite"}],"grant_file":"input-grant.json","timeout_ms":15000},
            {"program":support::adapter_binary("arcs-example-reasoner"),"profile":"solver-profile.json","args":[{"path":"solver.sqlite"}],"grant_file":"solver-grant.json","timeout_ms":15000}]}));
        f.add_input("wake", "schedule.events");
        f.add_input("other", "schedule.other");
        let mut runtime = f.read("runtime.json");
        runtime["runtime_id"] = json!("schedule-test");
        runtime["journal"] = json!("runtime.sqlite");
        runtime["max_jobs_per_tick"] = json!(10);
        runtime["max_pending_jobs"] = json!(1);
        runtime["routes"][0]["activation"] = activation;
        runtime["routes"][0]["budget"]["max_runtime_ms"] = json!(15000);
        f.write("runtime.json", runtime);
        f
    }
    fn add_input(&self, name: &str, id: &str) {
        let mut profile = self.read("input-profile.json");
        profile["adapter_id"] = json!(id);
        profile["events"] = json!([]);
        self.write(&format!("{name}-profile.json"), profile);
        let mut grant = self.read("input-grant.json");
        grant["adapter_id"] = json!(id);
        self.write(&format!("{name}-grant.json"), grant);
        let mut host = self.read("host.json");
        host["adapters"].as_array_mut().unwrap().push(json!({"program":support::adapter_binary("arcs-json-input"),"profile":format!("{name}-profile.json"),"args":[{"path":format!("{name}.sqlite")}],"grant_file":format!("{name}-grant.json"),"timeout_ms":15000}));
        self.write("host.json", host);
    }
    fn read(&self, file: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(file)).unwrap()).unwrap()
    }
    fn write(&self, file: &str, value: Value) {
        std::fs::write(
            self.directory.join(file),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn config(&self) -> RuntimeConfig {
        RuntimeConfig::load(self.directory.join("runtime.json")).unwrap()
    }
    fn recurring(&self, occurrences: u32) {
        let mut config = self.read("runtime.json");
        config["routes"][0]["recurrence"] =
            json!({"every_ms": MONTH, "max_occurrences": occurrences});
        self.write("runtime.json", config);
    }
    fn open(&self) -> (Host, TaskRuntime) {
        let host = Host::open(self.directory.join("host.json")).unwrap();
        let runtime = TaskRuntime::open(&host, self.config()).unwrap();
        (host, runtime)
    }
    fn enqueue(&self, input: &str, id: &str, subject: &str) {
        self.write("event.json", json!({"id":id,"subject":subject,"reference":format!("task:{id}"),"payload":{"values":[3,7],"target":6}}));
        let result = Command::new(support::adapter_binary("arcs-json-input"))
            .arg("--enqueue")
            .arg(self.directory.join(format!("{input}-profile.json")))
            .arg(self.directory.join(format!("{input}.sqlite")))
            .arg(self.directory.join("event.json"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fn invocations(&self) -> i64 {
        self.db("solver")
            .query_row("SELECT COUNT(*) FROM invocations", [], |row| row.get(0))
            .unwrap()
    }
    fn db(&self, name: &str) -> rusqlite::Connection {
        rusqlite::Connection::open(self.directory.join(format!("{name}.sqlite"))).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn event_activation() -> Value {
    json!({"kind":"on_event","source":{"adapter_id":"schedule.events","capability_id":"task.submit"},"schema_id":"arcs.task.nearest.v1","subject":"input:15:schedule.events:11:task.submit:11:project:one"})
}
fn assert_no_authority(host: &Host) {
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|event| !["action", "execution", "approval"]
                .contains(&event.artifact.artifact_type.as_str()))
    );
}

#[test]
fn three_month_timer_survives_restart_and_does_not_block_an_independent_task() {
    let due = NOW + 3 * MONTH;
    let f = Fixture::new(json!({"kind":"at_time","unix_ms":due}));
    let mut config = f.read("runtime.json");
    let mut immediate = config["routes"][0].clone();
    immediate["id"] = json!("immediate");
    immediate.as_object_mut().unwrap().remove("activation");
    config["routes"].as_array_mut().unwrap().push(immediate);
    config["max_pending_jobs"] = json!(2);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let tick = runtime.tick_at(&mut host, NOW).unwrap();
    assert_eq!(tick.ready_for_review.len(), 1);
    assert_eq!(tick.ready_for_review[0].route_id, "immediate");
    assert_eq!(runtime.report(0).unwrap().waiting_time, 1);
    assert_eq!(runtime.report(0).unwrap().next_wake_unix_ms, Some(due));
    assert_eq!(f.invocations(), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);

    let (mut host, mut runtime) = f.open();
    assert!(
        runtime
            .tick_at(&mut host, due - 1)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert_eq!(f.invocations(), 1);
    let done = runtime.tick_at(&mut host, due).unwrap().ready_for_review;
    assert_eq!(done.len(), 1);
    let schedule = runtime.scheduled_after(0, 10).unwrap().remove(0);
    assert_eq!(schedule.input_version_id, done[0].input_version_id);
    assert_eq!(schedule.state, ScheduleState::Ready);
    assert_eq!(schedule.released_at_unix_ms, Some(due));
    assert_eq!(runtime.report(0).unwrap().waiting_time, 0);
    assert_eq!(f.invocations(), 2);
    // A wall-clock rollback cannot put an already released task back to sleep.
    assert!(
        runtime
            .tick_at(&mut host, NOW)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn relative_deadline_is_frozen_at_acceptance_and_overdue_work_runs_once() {
    let f = Fixture::new(json!({"kind":"after_delay","delay_ms":MONTH}));
    let (mut host, mut runtime) = f.open();
    assert!(
        runtime
            .tick_at(&mut host, NOW)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert_eq!(
        runtime.scheduled_after(0, 1).unwrap()[0].not_before_unix_ms,
        Some(NOW + MONTH)
    );
    assert_eq!(f.invocations(), 0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    let done = runtime
        .tick_at(&mut host, NOW + 4 * MONTH)
        .unwrap()
        .ready_for_review;
    assert_eq!(done.len(), 1);
    assert_eq!(
        runtime.scheduled_after(0, 1).unwrap()[0].accepted_at_unix_ms,
        NOW
    );
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    // Crash after the Core result was saved but before the operational completion.
    f.db("runtime")
        .execute(
            "UPDATE runtime_jobs SET state='running',result_json=NULL",
            [],
        )
        .unwrap();
    let (mut host, mut runtime) = f.open();
    assert_eq!(
        runtime
            .tick_at(&mut host, NOW + 5 * MONTH)
            .unwrap()
            .ready_for_review,
        done
    );
    assert_eq!(f.invocations(), 1);
    assert_eq!(
        runtime.scheduled_after(0, 1).unwrap()[0].not_before_unix_ms,
        Some(NOW + MONTH)
    );
    host.shutdown().unwrap();
}

#[test]
fn a_full_queue_still_receives_its_wake_input_but_holds_unrelated_input() {
    let f = Fixture::new(event_activation());
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, NOW).unwrap();
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 1);
    assert_eq!(runtime.report(0).unwrap().waiting_event, 1);
    assert_eq!(f.invocations(), 0);
    f.enqueue("other", "unrelated", "project:one");
    f.enqueue("wake", "wrong-subject", "project:two");
    assert!(
        runtime
            .tick_at(&mut host, NOW + MONTH)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert_eq!(runtime.report(0).unwrap().waiting_event, 1);
    assert!(
        host.store()
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|event| event.artifact.created_by.id != "schedule.other")
    );
    f.enqueue("wake", "correct-subject", "project:one");
    let done = runtime
        .tick_at(&mut host, NOW + MONTH)
        .unwrap()
        .ready_for_review;
    assert_eq!(done.len(), 1);
    let saved = runtime.scheduled_after(0, 1).unwrap().remove(0);
    let wake = host
        .store()
        .get(&saved.wake_event_version_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(wake.created_by.id, "schedule.events");
    assert_eq!(wake.subject.unwrap().0, event_activation()["subject"]);
    // The wake payload grants no additional context or authority.
    let request = host
        .store()
        .get(&done[0].request_version_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        request.payload["invocation"]["context"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_no_authority(&host);
    assert_eq!(f.invocations(), 1);
    host.shutdown().unwrap();
}

#[test]
fn event_wait_replays_acked_events_after_restart_and_ignores_events_before_the_task() {
    let f = Fixture::new(event_activation());
    let mut profile = f.read("input-profile.json");
    profile["events"] = json!([]);
    f.write("input-profile.json", profile);
    let (mut host, mut runtime) = f.open();
    f.enqueue("wake", "old", "project:one");
    host.poll_inputs().unwrap();
    f.enqueue("input", "new-task", "project:one");
    runtime.tick_at(&mut host, NOW).unwrap();
    assert_eq!(runtime.report(0).unwrap().waiting_event, 1);
    // The external signal was ACKed to the Core, but the scheduler never saw it.
    f.enqueue("wake", "new", "project:one");
    let wake = host.poll_inputs().unwrap().remove(0).version_id;
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    assert_eq!(
        runtime
            .tick_at(&mut host, NOW + 2 * MONTH)
            .unwrap()
            .ready_for_review
            .len(),
        1
    );
    assert_eq!(
        runtime.scheduled_after(0, 1).unwrap()[0]
            .wake_event_version_id
            .as_ref(),
        Some(&wake)
    );
    assert_eq!(f.invocations(), 1);
    assert_eq!(runtime.report(0).unwrap().waiting_event, 0);
    host.shutdown().unwrap();
}

#[test]
fn bounded_event_scans_are_fair_across_restarts_and_do_not_lose_full_queue_backlog() {
    let f = Fixture::new(event_activation());
    let mut config = f.read("runtime.json");
    let mut second = config["routes"][0].clone();
    second["id"] = json!("second");
    config["routes"].as_array_mut().unwrap().push(second);
    config["max_pending_jobs"] = json!(2);
    config["max_jobs_per_tick"] = json!(1);
    config["max_events_per_tick"] = json!(1);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    for _ in 0..20 {
        runtime.tick_at(&mut host, NOW).unwrap();
    }
    assert_eq!(runtime.report(0).unwrap().waiting_event, 2);
    let before = runtime.scheduled_after(0, 10).unwrap();
    f.enqueue("input", "queued-behind-capacity", "project:one");
    f.enqueue("wake", "wake-both", "project:one");
    // Both events are in the Core even though discovery cannot admit the new task yet.
    host.poll_inputs().unwrap();
    runtime.tick_at(&mut host, NOW).unwrap();
    let after = runtime.scheduled_after(0, 10).unwrap();
    assert!(
        after[0].event_cursor > before[0].event_cursor
            || after[1].event_cursor > before[1].event_cursor
    );
    let cursor: i64 = f
        .db("runtime")
        .query_row("SELECT cursor FROM runtime_scheduler", [], |row| row.get(0))
        .unwrap();
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    let (mut host, mut runtime) = f.open();
    let other = if cursor == 1 { 1 } else { 0 };
    let old = runtime.scheduled_after(0, 10).unwrap()[other].event_cursor;
    runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    assert!(runtime.scheduled_after(0, 10).unwrap()[other].event_cursor > old);
    for _ in 0..80 {
        runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    }
    // Two inputs × two routes. The retained signal wakes tasks admitted later too.
    assert_eq!(runtime.report(0).unwrap().ready_for_review, 4);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert_eq!(f.invocations(), 4);
    assert_eq!(runtime.scheduled_after(0, 10).unwrap().len(), 4);
    host.shutdown().unwrap();
}

#[test]
fn invalid_activation_or_clock_cannot_create_a_job_or_skip_the_source_event() {
    for activation in [
        json!({"kind":"at_time","unix_ms":u64::MAX}),
        json!({"kind":"after_delay","delay_ms":315_576_000_001_u64}),
        json!({"kind":"on_event","source":{"adapter_id":"example.solver","capability_id":"solution.propose"},"schema_id":"arcs.task.nearest.v1"}),
        json!({"kind":"on_event","source":{"adapter_id":"schedule.events","capability_id":"task.submit"},"schema_id":"unknown"}),
    ] {
        let f = Fixture::new(activation);
        let mut host = Host::open(f.directory.join("host.json")).unwrap();
        assert!(TaskRuntime::open(&host, f.config()).is_err());
        assert!(!f.directory.join("runtime.sqlite").exists());
        host.shutdown().unwrap();
    }
    let f = Fixture::new(json!({"kind":"after_delay","delay_ms":1}));
    let (mut host, mut runtime) = f.open();
    assert!(runtime.tick_at(&mut host, u64::MAX).is_err());
    assert!(runtime.tick_at(&mut host, i64::MAX as u64).is_err());
    let input_sequence = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .find(|event| event.artifact.created_by.id == "example.input")
        .unwrap()
        .sequence;
    assert!(runtime.report(0).unwrap().source_cursor < input_sequence);
    assert_eq!(runtime.report(0).unwrap().pending_jobs, 0);
    assert!(runtime.scheduled_after(0, 0).is_err());
    assert_eq!(f.invocations(), 0);
    runtime.tick_at(&mut host, NOW).unwrap();
    assert_eq!(runtime.report(0).unwrap().waiting_time, 1);
    host.shutdown().unwrap();
}

#[test]
fn existing_journals_upgrade_without_changing_identity_or_replaying_completed_work() {
    let f = Fixture::new(Value::Null);
    let (mut host, mut runtime) = f.open();
    let done = runtime.tick_at(&mut host, NOW).unwrap().ready_for_review;
    assert_eq!(done.len(), 1);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    // Reproduce a journal created before scheduler tables were introduced.
    f.db("runtime")
        .execute_batch("DROP TABLE runtime_job_schedules; DROP TABLE runtime_scheduler;")
        .unwrap();
    let mut config = f.read("runtime.json");
    config["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("activation");
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    assert_eq!(runtime.completed_after(0, 10).unwrap(), done);
    assert!(
        runtime
            .tick_at(&mut host, NOW + MONTH)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert!(runtime.scheduled_after(0, 10).unwrap().is_empty());
    assert_eq!(f.invocations(), 1);
    host.shutdown().unwrap();
}

#[test]
fn an_authorized_observation_can_wake_a_task_when_its_queue_is_full() {
    let f = Fixture::new(
        json!({"kind":"on_event","source":{"adapter_id":"demo.sensor","capability_id":"temperature.observe"},"schema_id":"arcs.measurement.temperature.v1"}),
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut config = f.read("host.json");
    config["schemas"].as_array_mut().unwrap().push(json!(
        root.join("adapters/simulated-sensor/schemas/temperature.schema.json")
    ));
    config["adapters"].as_array_mut().unwrap().push(json!({"program":support::adapter_binary("arcs-simulated-sensor"),"profile":root.join("adapters/simulated-sensor/config/profile.json"),"grant_file":root.join("apps/adapter-host/config/grants/sensor.json"),"timeout_ms":15000}));
    f.write("host.json", config);
    let (mut host, mut runtime) = f.open();
    host.poll_inputs().unwrap();
    let done = runtime.tick_at(&mut host, NOW).unwrap().ready_for_review;
    assert_eq!(done.len(), 1);
    let saved = runtime.scheduled_after(0, 1).unwrap().remove(0);
    let event = host
        .store()
        .get(&saved.wake_event_version_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(event.created_by.id, "demo.sensor");
    assert_eq!(event.schema_id.0, "arcs.measurement.temperature.v1");
    assert_eq!(runtime.report(0).unwrap().waiting_event, 0);
    assert_no_authority(&host);
    host.shutdown().unwrap();
}

#[test]
fn a_legacy_single_activation_journal_keeps_its_due_date_and_receipt_when_upgraded() {
    let f = Fixture::new(json!({"kind":"after_delay","delay_ms":MONTH}));
    let (mut host, mut runtime) = f.open();
    runtime.tick_at(&mut host, NOW).unwrap();
    let original = runtime.scheduled_after(0, 1).unwrap().remove(0);
    host.shutdown().unwrap();
    drop(runtime);
    drop(host);
    f.db("runtime").execute_batch("BEGIN;
        DROP INDEX runtime_schedule_due;
        DROP INDEX runtime_schedule_open;
        ALTER TABLE runtime_job_schedules RENAME TO schedules_new;
        CREATE TABLE runtime_job_schedules(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),activation_json TEXT NOT NULL,accepted_at INTEGER NOT NULL CHECK(accepted_at>=0),not_before INTEGER CHECK(not_before>=0),event_adapter TEXT,event_cursor INTEGER NOT NULL CHECK(event_cursor>=0),released_at INTEGER CHECK(released_at>=0),wake_event_version TEXT);
        INSERT INTO runtime_job_schedules SELECT sequence,activation_json,accepted_at,not_before,event_adapter,event_cursor,released_at,wake_event_version FROM schedules_new;
        DROP TABLE schedules_new;
        CREATE INDEX runtime_schedule_due ON runtime_job_schedules(released_at,not_before,sequence);
        COMMIT;").unwrap();
    let (mut host, mut runtime) = f.open();
    let current = runtime
        .scheduled_history(original.sequence, None, 1)
        .unwrap()
        .remove(0);
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert!(
        runtime
            .tick_at(&mut host, NOW + MONTH - 1)
            .unwrap()
            .ready_for_review
            .is_empty()
    );
    assert_eq!(
        runtime
            .tick_at(&mut host, NOW + MONTH)
            .unwrap()
            .ready_for_review
            .len(),
        1
    );
    assert_eq!(f.invocations(), 1);
    assert!(runtime.scheduled_history(1, None, 0).is_err());
    assert!(runtime.scheduled_history(1, Some(0), 1).unwrap().is_empty());
    host.shutdown().unwrap();
}

#[test]
fn recurring_checks_resume_on_the_original_grid_and_coalesce_missed_dates() {
    let f = Fixture::new(Value::Null);
    f.recurring(5);
    let (mut host, mut runtime) = f.open();
    let first = runtime
        .tick_at(&mut host, NOW)
        .unwrap()
        .ready_for_review
        .remove(0);
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert_eq!(
        runtime.report(0).unwrap().next_wake_unix_ms,
        Some(NOW + MONTH)
    );
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    let late = NOW + 3 * MONTH + 11;
    let third = runtime
        .tick_at(&mut host, late)
        .unwrap()
        .ready_for_review
        .remove(0);
    assert_eq!(third.input_version_id, first.input_version_id);
    assert_ne!(third.request_version_id, first.request_version_id);
    let receipt = runtime
        .check_occurrences_after(first.sequence, first.sequence, 1)
        .unwrap()
        .remove(0);
    assert_eq!(receipt.occurrence, 3);
    assert_eq!(receipt.sequence, third.sequence);
    assert_eq!(receipt.due_unix_ms, NOW + 3 * MONTH);
    assert_eq!(receipt.admitted_at_unix_ms, late);
    assert_eq!(receipt.skipped_before, 2);
    let request = host
        .store()
        .get(&third.request_version_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        request.payload["invocation"]["constraints"]["task_recurrence"],
        json!(receipt)
    );
    for time in [late, NOW + MONTH, NOW + 4 * MONTH - 1] {
        assert!(
            runtime
                .tick_at(&mut host, time)
                .unwrap()
                .ready_for_review
                .is_empty()
        );
    }
    assert_eq!(f.invocations(), 2);
    let last = runtime
        .tick_at(&mut host, NOW + 4 * MONTH)
        .unwrap()
        .ready_for_review
        .remove(0);
    let summary = runtime.report(0).unwrap();
    assert_eq!(summary.next_wake_unix_ms, None);
    assert_eq!(summary.recurring_checks[0].skipped_occurrences, 2);
    assert_eq!(summary.recurring_checks[0].last_sequence, last.sequence);
    assert_eq!(
        runtime
            .check_occurrences_after(first.sequence, 0, 10)
            .unwrap()
            .len(),
        3
    );
    assert!(
        runtime
            .recurring_checks_after(first.sequence, 1)
            .unwrap()
            .is_empty()
    );
    assert_no_authority(&host);
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    assert!(
        runtime
            .tick_at(&mut host, NOW + 8 * MONTH)
            .unwrap()
            .recurring_jobs
            .is_empty()
    );
    assert_eq!(
        runtime.completed_after(0, 10).unwrap(),
        [first, third, last]
    );
    assert_eq!(f.invocations(), 3);
    host.shutdown().unwrap();
}

#[test]
fn recurring_admission_rolls_back_and_an_open_occurrence_blocks_the_next() {
    let f = Fixture::new(Value::Null);
    f.recurring(4);
    let mut config = f.read("runtime.json");
    config["max_pending_jobs"] = json!(2);
    f.write("runtime.json", config);
    let (mut host, mut runtime) = f.open();
    let first = runtime
        .tick_at(&mut host, NOW)
        .unwrap()
        .ready_for_review
        .remove(0);
    let db = f.db("runtime");
    db.execute_batch(
        "CREATE TRIGGER fail_occurrence BEFORE INSERT ON runtime_check_occurrences
        WHEN NEW.occurrence>0 BEGIN SELECT RAISE(ABORT,'test occurrence admission failure'); END;",
    )
    .unwrap();
    assert!(runtime.tick_at(&mut host, NOW + MONTH).is_err());
    assert_eq!(
        runtime.report(0).unwrap().next_wake_unix_ms,
        Some(NOW + MONTH)
    );
    assert_eq!(runtime.report(0).unwrap().capacity_used, 0);
    assert_eq!(f.invocations(), 1);
    db.execute_batch(
        "DROP TRIGGER fail_occurrence;
        CREATE TRIGGER fail_completion BEFORE UPDATE ON runtime_jobs
        WHEN NEW.route_id LIKE 'check:%' AND NEW.state='ready_for_review'
        BEGIN SELECT RAISE(ABORT,'test operational completion failure'); END;",
    )
    .unwrap();
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    assert!(runtime.tick_at(&mut host, NOW + MONTH).is_err());
    let admitted = runtime
        .check_occurrences_after(first.sequence, first.sequence, 10)
        .unwrap();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].occurrence, 1);
    assert_eq!(f.invocations(), 2);
    host.shutdown().unwrap();
    drop((runtime, host));
    db.execute_batch("DROP TRIGGER fail_completion").unwrap();

    let (mut host, mut runtime) = f.open();
    let recovered = runtime.tick_at(&mut host, NOW + 3 * MONTH).unwrap();
    assert!(recovered.recurring_jobs.is_empty());
    assert_eq!(recovered.ready_for_review[0].sequence, admitted[0].sequence);
    assert_eq!(f.invocations(), 2);
    let next = runtime.tick_at(&mut host, NOW + 3 * MONTH).unwrap();
    assert_eq!(next.ready_for_review.len(), 1);
    let history = runtime
        .check_occurrences_after(first.sequence, 0, 10)
        .unwrap();
    assert_eq!(
        history.iter().map(|row| row.occurrence).collect::<Vec<_>>(),
        [0, 1, 3]
    );
    assert_eq!(history[2].skipped_before, 1);
    assert_eq!(f.invocations(), 3);
    host.shutdown().unwrap();
}

#[test]
fn new_tasks_and_recurring_checks_share_a_single_queue_slot_across_restart() {
    let f = Fixture::new(Value::Null);
    f.recurring(5);
    let (mut host, mut runtime) = f.open();
    let first = runtime
        .tick_at(&mut host, NOW)
        .unwrap()
        .ready_for_review
        .remove(0);
    f.enqueue("input", "second", "project:second");
    let second = runtime.tick_at(&mut host, NOW + MONTH).unwrap();
    assert!(second.recurring_jobs.is_empty());
    assert_eq!(second.ready_for_review.len(), 1);
    assert_ne!(
        second.ready_for_review[0].input_version_id,
        first.input_version_id
    );
    host.shutdown().unwrap();
    drop((runtime, host));

    let (mut host, mut runtime) = f.open();
    f.enqueue("input", "third", "project:third");
    let recurrence = runtime.tick_at(&mut host, NOW + 2 * MONTH).unwrap();
    assert_eq!(recurrence.recurring_jobs.len(), 1);
    assert_eq!(recurrence.ready_for_review.len(), 1);
    assert_eq!(
        recurrence.ready_for_review[0].input_version_id,
        first.input_version_id
    );
    let third = runtime.tick_at(&mut host, NOW + 2 * MONTH).unwrap();
    assert_eq!(third.discovered_jobs, 1);
    assert!(third.recurring_jobs.is_empty());
    assert_eq!(third.ready_for_review.len(), 1);
    assert_eq!(runtime.recurring_checks_after(0, 10).unwrap().len(), 3);
    assert_eq!(f.invocations(), 4);
    host.shutdown().unwrap();
}

#[test]
fn invalid_recurrence_is_rejected_before_any_work_is_admitted() {
    let f = Fixture::new(Value::Null);
    f.recurring(4);
    let config = f.read("runtime.json");
    let mut host = Host::open(f.directory.join("host.json")).unwrap();
    for (field, value) in [
        ("recurrence", json!({"every_ms":0,"max_occurrences":4})),
        (
            "recurrence",
            json!({"every_ms":MONTH,"max_occurrences":10_001}),
        ),
        ("activation", json!({"kind":"after_delay","delay_ms":1})),
        (
            "allowed_capabilities",
            json!([{"adapter_id":"example.solver","capability_id":"solution.propose"}]),
        ),
        ("constraints", json!({"task_recurrence":null})),
    ] {
        let mut invalid = config.clone();
        invalid["routes"][0][field] = value;
        f.write("runtime.json", invalid);
        assert!(TaskRuntime::open(&host, f.config()).is_err(), "{field}");
        assert!(!f.directory.join("runtime.sqlite").exists());
    }
    assert_eq!(f.invocations(), 0);
    host.shutdown().unwrap();
}

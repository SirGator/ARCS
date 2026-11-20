use super::*;

fn setup() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE runtime_jobs(sequence INTEGER PRIMARY KEY,route_id TEXT,input_version TEXT,state TEXT,goal_json TEXT);
        CREATE TABLE runtime_plan_steps(sequence INTEGER,parent_sequence INTEGER,template_id TEXT);
        CREATE TABLE runtime_job_schedules(sequence INTEGER,released_at INTEGER);
        CREATE TABLE runtime_job_joins(sequence INTEGER,state TEXT);").unwrap();
    let config: RuntimeConfig = serde_json::from_value(
        serde_json::json!({"config_version":1,"runtime_id":"test","journal":"unused","routes":[]}),
    )
    .unwrap();
    super::super::reasoning::initialize(&connection).unwrap();
    super::super::suspension::initialize(&connection).unwrap();
    super::super::recurrence::initialize(&connection).unwrap();
    initialize(&connection, &config).unwrap();
    connection
}

fn add(connection: &Connection, sequence: i64, priority: u8, state: &str) {
    connection
        .execute(
            "INSERT INTO runtime_jobs(sequence,route_id,input_version,state) VALUES(?1,'route','input',?2)",
            params![sequence, state],
        )
        .unwrap();
    Journal::assign_priority(connection, sequence as u64, priority).unwrap();
}

fn tick(connection: &Connection) -> SelectedTask {
    connection
        .execute("DELETE FROM runtime_dispatch_visited", [])
        .unwrap();
    select(connection).unwrap().unwrap()
}

#[test]
fn weighted_service_is_bounded_and_each_class_and_job_makes_progress() {
    let connection = setup();
    for priority in 0..4 {
        for job in 1..=3 {
            add(
                &connection,
                3 * i64::from(priority) + job,
                priority,
                "pending",
            );
        }
    }
    let mut counts = [0; 4];
    let mut per_job = [0; 12];
    let mut low_gap = 0;
    for _ in 0..120 {
        let task = tick(&connection);
        counts[task.3 as usize] += 1;
        per_job[task.0 as usize - 1] += 1;
        if task.3 == 0 {
            low_gap = 0;
        } else {
            low_gap += 1;
        }
        assert!(low_gap < 15, "low-priority class starved");
    }
    assert_eq!(counts, [8, 16, 32, 64]);
    for group in per_job.as_chunks::<3>().0 {
        assert!(group.iter().max().unwrap() - group.iter().min().unwrap() <= 1);
    }
}

#[test]
fn one_visit_per_tick_and_ineligible_classes_do_not_spend_the_budget() {
    let connection = setup();
    add(&connection, 1, 3, "awaiting_approval");
    add(&connection, 2, 3, "pending");
    add(&connection, 3, 2, "pending");
    add(&connection, 4, 0, "pending");
    add(&connection, 5, 0, "cancelled");
    add(&connection, 6, 1, "failed");
    connection
        .execute("INSERT INTO runtime_job_schedules VALUES(2,NULL)", [])
        .unwrap();
    connection
        .execute("INSERT INTO runtime_job_joins VALUES(3,'waiting')", [])
        .unwrap();
    // Review checkpoints must still reconcile decisions already committed to
    // the Core, even when their queue checkpoint was lost in a crash.
    assert_eq!(select(&connection).unwrap().unwrap().0, 1);
    assert_eq!(select(&connection).unwrap().unwrap().0, 4);
    assert!(select(&connection).unwrap().is_none());
    connection
        .execute("UPDATE runtime_job_schedules SET released_at=1", [])
        .unwrap();
    connection
        .execute("UPDATE runtime_job_joins SET state='ready'", [])
        .unwrap();
    let mut ids = vec![
        select(&connection).unwrap().unwrap().0,
        select(&connection).unwrap().unwrap().0,
    ];
    ids.sort();
    assert_eq!(ids, [2, 3]);
    assert!(select(&connection).unwrap().is_none());
}

#[test]
fn continuing_arrivals_cannot_starve_previously_selected_running_jobs() {
    let connection = setup();
    add(&connection, 1, 0, "running");
    add(&connection, 2, 0, "running");
    let mut original_turns = [0; 2];
    let mut seen_new_job = false;
    for next in 3..80 {
        add(&connection, next, 0, "pending");
        let chosen = tick(&connection).0;
        if chosen <= 2 {
            original_turns[chosen as usize - 1] += 1;
        } else {
            seen_new_job = true;
        }
    }
    assert!(original_turns.iter().all(|turns| *turns >= 3));
    assert!(seen_new_job);
}

#[test]
fn a_failed_cursor_commit_rolls_back_the_whole_turn() {
    let connection = setup();
    add(&connection, 1, 3, "pending");
    connection.execute_batch("CREATE TEMP TRIGGER fail_selection BEFORE INSERT ON runtime_dispatch_visited BEGIN SELECT RAISE(ABORT,'injected commit failure'); END;").unwrap();
    assert!(select(&connection).is_err());
    assert_eq!(
        connection
            .query_row("SELECT next_slot FROM runtime_dispatch_state", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT SUM(selections) FROM runtime_dispatch_classes",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute_batch("DROP TRIGGER fail_selection;")
        .unwrap();
    assert_eq!(select(&connection).unwrap().unwrap().0, 1);
}

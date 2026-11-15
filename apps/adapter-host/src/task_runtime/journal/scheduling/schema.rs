//! Additive Migration der einmaligen Aktivierung zu mehreren Wartebelegen.
use rusqlite::Connection;

use super::{HostError, db};

const TABLE: &str = "CREATE TABLE IF NOT EXISTS runtime_job_schedules(
    sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0),
    activation_json TEXT NOT NULL,
    accepted_at INTEGER NOT NULL CHECK(accepted_at>=0),
    not_before INTEGER CHECK(not_before>=0),
    event_adapter TEXT,
    event_cursor INTEGER NOT NULL CHECK(event_cursor>=0),
    released_at INTEGER CHECK(released_at>=0),
    wake_event_version TEXT,
    PRIMARY KEY(sequence,revision));";

pub(in crate::task_runtime::journal) fn initialize(
    connection: &Connection,
) -> Result<(), HostError> {
    let transaction = connection.unchecked_transaction().map_err(db)?;
    transaction.execute_batch(TABLE).map_err(db)?;
    let revision: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('runtime_job_schedules') WHERE name='revision')",
        [], |row| row.get(0),
    ).map_err(db)?;
    if !revision {
        transaction
            .execute_batch(
                "DROP INDEX IF EXISTS runtime_schedule_due;
            ALTER TABLE runtime_job_schedules RENAME TO runtime_job_schedules_previous;",
            )
            .map_err(db)?;
        transaction.execute_batch(TABLE).map_err(db)?;
        transaction.execute_batch("INSERT INTO runtime_job_schedules(sequence,activation_json,accepted_at,not_before,event_adapter,event_cursor,released_at,wake_event_version)
            SELECT sequence,activation_json,accepted_at,not_before,event_adapter,event_cursor,released_at,wake_event_version FROM runtime_job_schedules_previous;
            DROP TABLE runtime_job_schedules_previous;").map_err(db)?;
    }
    transaction.execute_batch("CREATE INDEX IF NOT EXISTS runtime_schedule_due ON runtime_job_schedules(released_at,not_before,sequence);
        CREATE UNIQUE INDEX IF NOT EXISTS runtime_schedule_open ON runtime_job_schedules(sequence) WHERE released_at IS NULL;
        CREATE TABLE IF NOT EXISTS runtime_scheduler(singleton INTEGER PRIMARY KEY CHECK(singleton=1),cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0));
        INSERT OR IGNORE INTO runtime_scheduler(singleton) VALUES(1);").map_err(db)?;
    transaction.commit().map_err(db)
}

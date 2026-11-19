//! Operational time grid and occurrence receipts. Reasoning evidence stays in Core.
mod control;
use arcs::core::VersionId;
use rusqlite::{Connection, Row, Transaction, params};

use super::{Journal, active, capacity_query, db};
use crate::HostError;
use crate::task_runtime::{
    RuntimeConfig,
    config::invalid,
    scheduler::{CheckOccurrence, CheckSeries, Recurrence, SeriesState, occurrence_route_id},
};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_check_series(
            sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),
            anchor_at INTEGER NOT NULL CHECK(anchor_at>=0),
            every_ms INTEGER NOT NULL CHECK(every_ms>0),
            max_occurrences INTEGER NOT NULL CHECK(max_occurrences BETWEEN 2 AND 10000),
            next_occurrence INTEGER NOT NULL CHECK(next_occurrence BETWEEN 1 AND max_occurrences),
            next_due INTEGER CHECK(next_due>anchor_at),
            skipped INTEGER NOT NULL DEFAULT 0 CHECK(skipped>=0),
            last_job INTEGER NOT NULL REFERENCES runtime_jobs(sequence),
            control_revision INTEGER NOT NULL DEFAULT 0 CHECK(control_revision>=0),
            control_state TEXT NOT NULL DEFAULT 'active' CHECK(control_state IN ('active','paused','cancelled')),
            control_reason TEXT,
            CHECK((next_due IS NULL)=(next_occurrence=max_occurrences)));
        CREATE TABLE IF NOT EXISTS runtime_check_occurrences(
            sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),
            series INTEGER NOT NULL REFERENCES runtime_check_series(sequence),
            occurrence INTEGER NOT NULL CHECK(occurrence>=0),
            due_at INTEGER NOT NULL CHECK(due_at>=0),
            admitted_at INTEGER NOT NULL CHECK(admitted_at>=due_at),
            skipped_before INTEGER NOT NULL CHECK(skipped_before>=0),
            UNIQUE(series,occurrence));
        CREATE INDEX IF NOT EXISTS runtime_check_history ON runtime_check_occurrences(series,sequence);
        CREATE TABLE IF NOT EXISTS runtime_check_admission(
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            checks_first INTEGER NOT NULL CHECK(checks_first IN (0,1)));
        INSERT OR IGNORE INTO runtime_check_admission VALUES(1,0);
        COMMIT;").map_err(db)?;
    control::initialize(connection)
}

const SERIES_ENABLED: &str = "s.control_state='active'";

fn series_select(include_failed: bool) -> String {
    let last_active = active(include_failed).replace("state", "last.state");
    format!(
        "SELECT s.sequence, origin.route_id, origin.input_version,
        s.anchor_at,s.every_ms,s.max_occurrences,s.next_due,s.skipped,s.last_job,last.state,
        s.control_state,s.control_revision,s.control_reason,{last_active}
        FROM runtime_check_series s JOIN runtime_jobs origin ON origin.sequence=s.sequence
        JOIN runtime_jobs last ON last.sequence=s.last_job"
    )
}

fn read_series(row: &Row<'_>) -> rusqlite::Result<CheckSeries> {
    let next_due = row.get::<_, Option<i64>>(6)?.map(|due| due as u64);
    let unfinished = next_due.is_some() || row.get::<_, bool>(13)?;
    let state = match row.get::<_, String>(10)?.as_str() {
        "cancelled" => SeriesState::Cancelled,
        "active" | "paused" if !unfinished => SeriesState::Exhausted,
        "active" => SeriesState::Active,
        "paused" => SeriesState::Paused,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(CheckSeries {
        sequence: row.get::<_, i64>(0)? as u64,
        route_id: row.get(1)?,
        input_version_id: VersionId(row.get(2)?),
        anchor_unix_ms: row.get::<_, i64>(3)? as u64,
        every_ms: row.get::<_, i64>(4)? as u64,
        max_occurrences: row.get(5)?,
        next_due_unix_ms: next_due.filter(|_| state != SeriesState::Cancelled),
        skipped_occurrences: row.get(7)?,
        last_sequence: row.get::<_, i64>(8)? as u64,
        last_state: row.get(9)?,
        state,
        control_revision: row.get::<_, i64>(11)? as u64,
        control_reason: row.get(12)?,
    })
}

fn read_occurrence(row: &Row<'_>) -> rusqlite::Result<CheckOccurrence> {
    Ok(CheckOccurrence {
        series_sequence: row.get::<_, i64>(0)? as u64,
        occurrence: row.get(1)?,
        sequence: row.get::<_, i64>(2)? as u64,
        due_unix_ms: row.get::<_, i64>(3)? as u64,
        admitted_at_unix_ms: row.get::<_, i64>(4)? as u64,
        skipped_before: row.get(5)?,
    })
}

impl Journal {
    pub(in crate::task_runtime) fn start_recurrence(
        tx: &Transaction<'_>,
        sequence: u64,
        recurrence: Recurrence,
        now: u64,
    ) -> Result<(), HostError> {
        recurrence.due(now, recurrence.max_occurrences - 1)?;
        let next_due = recurrence.due(now, 1)?;
        tx.execute(
            "INSERT INTO runtime_check_series(sequence,anchor_at,every_ms,max_occurrences,next_occurrence,next_due,last_job)
             VALUES(?1,?2,?3,?4,1,?5,?1)",
            params![sequence as i64, now as i64, recurrence.every_ms as i64, recurrence.max_occurrences, next_due as i64],
        ).map_err(db)?;
        tx.execute(
            "INSERT INTO runtime_check_occurrences VALUES(?1,?1,0,?2,?2,0)",
            params![sequence as i64, now as i64],
        )
        .map_err(db)?;
        Ok(())
    }

    /// Give both sources a turn at a free slot, even with a queue of size one.
    pub(in crate::task_runtime) fn recurring_first(
        &self,
        config: &RuntimeConfig,
    ) -> Result<bool, HostError> {
        if self.capacity(config.retry_failed_jobs())? >= config.max_pending_jobs {
            return Ok(false);
        }
        self.connection.query_row(
            "UPDATE runtime_check_admission SET checks_first=1-checks_first RETURNING checks_first=0",
            [], |row| row.get(0),
        ).map_err(db)
    }

    pub(in crate::task_runtime) fn admit_recurring_checks(
        &mut self,
        config: &RuntimeConfig,
        now: u64,
    ) -> Result<Vec<u64>, HostError> {
        let include_failed = config.retry_failed_jobs();
        let tx = self.connection.transaction().map_err(db)?;
        let used: usize = tx
            .query_row(&capacity_query(include_failed), [], |row| {
                row.get::<_, i64>(0).map(|used| used as usize)
            })
            .map_err(db)?;
        let limit = config
            .max_pending_jobs
            .saturating_sub(used)
            .min(config.max_jobs_per_tick);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let due = {
            let active_last = active(include_failed).replace("state", "last.state");
            let mut query = tx
                .prepare(&format!(
                    "SELECT s.sequence,origin.route_id,origin.input_version,origin.source_sequence,
                    s.anchor_at,s.next_occurrence
                 FROM runtime_check_series s JOIN runtime_jobs last ON last.sequence=s.last_job
                 JOIN runtime_jobs origin ON origin.sequence=s.sequence
                 WHERE s.next_due<=?1 AND {SERIES_ENABLED} AND NOT ({active_last})
                 ORDER BY s.next_due,s.sequence LIMIT ?2"
                ))
                .map_err(db)?;
            query
                .query_map(params![now as i64, limit as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)? as u64,
                        row.get::<_, i64>(4)? as u64,
                        row.get::<_, u32>(5)?,
                    ))
                })
                .map_err(db)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db)?
        };
        let mut admitted = Vec::with_capacity(due.len());
        for (series, route_id, input, source, anchor, next) in due {
            let route = config
                .routes
                .iter()
                .find(|route| route.id == route_id)
                .ok_or_else(|| invalid("recurring check origin route is missing"))?;
            let recurrence = route
                .recurrence
                .ok_or_else(|| invalid("recurrence policy missing"))?;
            let elapsed = now
                .checked_sub(anchor)
                .ok_or_else(|| invalid("recurrence anchor is after its due date"))?;
            let occurrence = (elapsed / recurrence.every_ms)
                .min(u64::from(recurrence.max_occurrences - 1)) as u32;
            let skipped = occurrence
                .checked_sub(next)
                .ok_or_else(|| invalid("invalid recurring check cursor"))?;
            let due_at = recurrence.due(anchor, occurrence)?;
            let next_due = (occurrence + 1 < recurrence.max_occurrences)
                .then(|| recurrence.due(anchor, occurrence + 1))
                .transpose()?;
            tx.execute(
                "INSERT INTO runtime_jobs(route_id,input_version,source_sequence,state) VALUES(?1,?2,?3,'pending')",
                params![occurrence_route_id(series, occurrence), input, source as i64],
            ).map_err(db)?;
            let sequence = tx.last_insert_rowid() as u64;
            Self::assign_priority(&tx, sequence, route.priority)?;
            Self::assign_deadline(&tx, sequence, route.max_lifetime_ms, false, None)?;
            tx.execute(
                "INSERT INTO runtime_check_occurrences VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    sequence as i64,
                    series as i64,
                    occurrence,
                    due_at as i64,
                    now as i64,
                    skipped
                ],
            )
            .map_err(db)?;
            tx.execute("UPDATE runtime_check_series SET next_occurrence=?2,next_due=?3,skipped=skipped+?4,last_job=?5 WHERE sequence=?1",
                params![series as i64, occurrence + 1, next_due.map(|due| due as i64), skipped, sequence as i64]).map_err(db)?;
            admitted.push(sequence);
        }
        tx.commit().map_err(db)?;
        Ok(admitted)
    }

    pub(in crate::task_runtime) fn check_origin(
        &self,
        sequence: u64,
    ) -> Result<(String, CheckOccurrence), HostError> {
        self.connection.query_row(
            "SELECT o.series,o.occurrence,o.sequence,o.due_at,o.admitted_at,o.skipped_before,origin.route_id
             FROM runtime_check_occurrences o JOIN runtime_jobs origin ON origin.sequence=o.series
             JOIN runtime_jobs job ON job.sequence=o.sequence
             WHERE o.sequence=?1 AND job.input_version=origin.input_version AND job.source_sequence=origin.source_sequence",
            [sequence as i64], |row| Ok((row.get(6)?, read_occurrence(row)?)),
        ).map_err(db)
    }

    pub(in crate::task_runtime) fn next_check(&self) -> Result<Option<u64>, HostError> {
        self.connection
            .query_row(
                &format!("SELECT MIN(s.next_due) FROM runtime_check_series s WHERE s.next_due IS NOT NULL AND {SERIES_ENABLED}"),
                [],
                |row| {
                    row.get::<_, Option<i64>>(0)
                        .map(|due| due.map(|due| due as u64))
                },
            )
            .map_err(db)
    }

    pub(in crate::task_runtime) fn recurring_checks_for_report(
        &self,
        include_failed: bool,
    ) -> Result<Vec<CheckSeries>, HostError> {
        let mut query = self.connection.prepare(&format!(
            "{} ORDER BY (s.next_due IS NULL OR NOT ({SERIES_ENABLED})), s.next_due, s.sequence DESC LIMIT 50",
            series_select(include_failed)
        )).map_err(db)?;
        let rows = query.query_map([], read_series).map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }

    pub(in crate::task_runtime) fn recurring_checks_after(
        &self,
        sequence: u64,
        limit: usize,
        include_failed: bool,
    ) -> Result<Vec<CheckSeries>, HostError> {
        validate_page(limit)?;
        let mut query = self
            .connection
            .prepare(&format!(
                "{} WHERE s.sequence>?1 ORDER BY s.sequence LIMIT ?2",
                series_select(include_failed)
            ))
            .map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                read_series,
            )
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }

    pub(in crate::task_runtime) fn check_occurrences_after(
        &self,
        series: u64,
        after: u64,
        limit: usize,
    ) -> Result<Vec<CheckOccurrence>, HostError> {
        validate_page(limit)?;
        let mut query = self.connection.prepare(
            "SELECT series,occurrence,sequence,due_at,admitted_at,skipped_before
             FROM runtime_check_occurrences WHERE series=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3"
        ).map_err(db)?;
        let rows = query
            .query_map(
                params![
                    series.min(i64::MAX as u64) as i64,
                    after.min(i64::MAX as u64) as i64,
                    limit as i64
                ],
                read_occurrence,
            )
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }
}

fn validate_page(limit: usize) -> Result<(), HostError> {
    if !(1..=1000).contains(&limit) {
        return Err(invalid("recurring check page size must be 1..1000"));
    }
    Ok(())
}

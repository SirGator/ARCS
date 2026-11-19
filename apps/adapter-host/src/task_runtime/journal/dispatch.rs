//! Dauerhafter, gewichteter Umlauf für begrenzte Aufgabenschritte.
//! Ein Turn ist ein Ausführungsversuch, keine Zusage freier Adapterressourcen.
use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{RuntimeConfig, config::invalid};
use arcs::core::VersionId;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

// Interleaved weights 1:2:4:8; every class has a slot in every full cycle.
const WHEEL: [u8; 15] = [3, 2, 3, 1, 3, 2, 3, 0, 3, 2, 3, 1, 3, 2, 3];
type SelectedTask = (u64, String, VersionId, u8);

#[derive(Debug, Serialize)]
pub struct DispatchReport {
    pub policy: &'static str,
    pub next_slot: usize,
    pub classes: Vec<DispatchClassReport>,
}

#[derive(Debug, Serialize)]
pub struct DispatchClassReport {
    pub priority: u8,
    pub weight: u8,
    pub selections: u64,
    pub last_sequence: u64,
    pub round_end: u64,
}

pub(super) fn initialize(connection: &Connection, config: &RuntimeConfig) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_dispatch_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),next_slot INTEGER NOT NULL CHECK(next_slot BETWEEN 0 AND 14));
        INSERT OR IGNORE INTO runtime_dispatch_state VALUES(1,0);
        CREATE TABLE IF NOT EXISTS runtime_dispatch_classes(priority INTEGER PRIMARY KEY CHECK(priority BETWEEN 0 AND 3),cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0),round_end INTEGER NOT NULL DEFAULT 0 CHECK(round_end>=0),selections INTEGER NOT NULL DEFAULT 0 CHECK(selections>=0));
        INSERT OR IGNORE INTO runtime_dispatch_classes(priority) VALUES(0),(1),(2),(3);
        CREATE TABLE IF NOT EXISTS runtime_job_priorities(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),priority INTEGER NOT NULL CHECK(priority BETWEEN 0 AND 3));
        CREATE INDEX IF NOT EXISTS runtime_priority_jobs ON runtime_job_priorities(priority,sequence);
        CREATE TEMP TABLE IF NOT EXISTS runtime_dispatch_visited(sequence INTEGER PRIMARY KEY);
        COMMIT;").map_err(db)?;
    // Additive migration: derive old job priorities only from the bound operator
    // definition, including dynamic templates. The model cannot supply priorities.
    let transaction = connection.unchecked_transaction().map_err(db)?;
    for route in &config.routes {
        transaction.execute(
            "INSERT OR IGNORE INTO runtime_job_priorities SELECT sequence,?2 FROM runtime_jobs WHERE route_id=?1",
            params![route.id,route.priority],
        ).map_err(db)?;
        if let Some(plan) = &route.plan {
            for template in &plan.templates {
                transaction.execute(
                    "INSERT OR IGNORE INTO runtime_job_priorities SELECT s.sequence,?3 FROM runtime_plan_steps s JOIN runtime_jobs parent ON parent.sequence=s.parent_sequence WHERE parent.route_id=?1 AND s.template_id=?2",
                    params![route.id,template.route.id,template.route.priority],
                ).map_err(db)?;
            }
        }
    }
    let missing: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM runtime_jobs j LEFT JOIN runtime_job_priorities p ON p.sequence=j.sequence WHERE p.sequence IS NULL)",
        [], |row| row.get(0),
    ).map_err(db)?;
    if missing {
        return Err(invalid("saved task has no authorized priority definition"));
    }
    transaction.commit().map_err(db)
}

impl Journal {
    pub(in crate::task_runtime) fn assign_priority(
        connection: &Connection,
        sequence: u64,
        priority: u8,
    ) -> Result<(), HostError> {
        connection
            .execute(
                "INSERT INTO runtime_job_priorities VALUES(?1,?2)",
                params![sequence as i64, priority],
            )
            .map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn begin_dispatch_tick(&self) -> Result<(), HostError> {
        self.connection
            .execute("DELETE FROM runtime_dispatch_visited", [])
            .map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn next(
        &self,
        include_failed: bool,
    ) -> Result<Option<SelectedTask>, HostError> {
        select(&self.connection, include_failed)
    }

    pub(in crate::task_runtime) fn dispatch_report(&self) -> Result<DispatchReport, HostError> {
        let next_slot = self
            .connection
            .query_row(
                "SELECT next_slot FROM runtime_dispatch_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0).map(|n| n as usize),
            )
            .map_err(db)?;
        let mut query = self
            .connection
            .prepare(
                "SELECT priority,selections,cursor,round_end FROM runtime_dispatch_classes ORDER BY priority",
            )
            .map_err(db)?;
        let classes = query
            .query_map([], |row| {
                let priority: u8 = row.get(0)?;
                Ok(DispatchClassReport {
                    priority,
                    weight: 1 << priority,
                    selections: row.get::<_, i64>(1)? as u64,
                    last_sequence: row.get::<_, i64>(2)? as u64,
                    round_end: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect::<Result<_, _>>()?;
        Ok(DispatchReport {
            policy: "weighted_round_robin_v1",
            next_slot,
            classes,
        })
    }
}

fn select(
    connection: &Connection,
    include_failed: bool,
) -> Result<Option<SelectedTask>, HostError> {
    // Only operational scheduling metadata is locked; commit precedes any Core
    // access or external I/O. A crash may consume one turn, never an extra action.
    let transaction = connection.unchecked_transaction().map_err(db)?;
    let slot: usize = transaction
        .query_row(
            "SELECT next_slot FROM runtime_dispatch_state WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0).map(|n| n as usize),
        )
        .map_err(db)?;
    let runnable = super::runnable("j", include_failed);
    let sql = format!("SELECT j.sequence,j.route_id,j.input_version FROM runtime_job_priorities p JOIN runtime_jobs j ON j.sequence=p.sequence WHERE p.priority=?1 AND {runnable}
        AND NOT EXISTS(SELECT 1 FROM runtime_dispatch_visited v WHERE v.sequence=j.sequence)
        AND NOT EXISTS(SELECT 1 FROM runtime_job_schedules s WHERE s.sequence=j.sequence AND s.released_at IS NULL)
        AND NOT EXISTS(SELECT 1 FROM runtime_job_joins d WHERE d.sequence=j.sequence AND d.state!='ready')
        AND NOT ({})
        AND p.sequence > ?2 AND p.sequence <= ?3 ORDER BY p.sequence LIMIT 1",super::suspension::PAUSED);
    let mut selected = None;
    {
        let mut query = transaction.prepare(&sql).map_err(db)?;
        let mut empty = [false; 4];
        // Each distinct class needs at most two indexed range queries, avoiding
        // a sort of its entire job history. Empty classes are skipped for this
        // selection only, so a newly ready class retains its future turns.
        for offset in 0..WHEEL.len() {
            let current_slot = (slot + offset) % WHEEL.len();
            let priority = WHEEL[current_slot];
            if empty[priority as usize] {
                continue;
            }
            let (cursor, mut round_end): (i64, i64) = transaction
                .query_row(
                    "SELECT cursor,round_end FROM runtime_dispatch_classes WHERE priority=?1",
                    [priority],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(db)?;
            let read = |row: &rusqlite::Row<'_>| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, String>(1)?,
                    VersionId(row.get(2)?),
                    priority,
                ))
            };
            let task = match query
                .query_row(params![priority, cursor, round_end], read)
                .optional()
                .map_err(db)?
            {
                Some(task) => Some(task),
                None => {
                    // Freeze the next cohort. A stream of newly appended jobs
                    // cannot indefinitely postpone another turn for old workers.
                    round_end = transaction.query_row(
                        "SELECT COALESCE(MAX(sequence),0) FROM runtime_job_priorities WHERE priority=?1",[priority],|row|row.get(0),
                    ).map_err(db)?;
                    query
                        .query_row(params![priority, 0, round_end], read)
                        .optional()
                        .map_err(db)?
                }
            };
            if let Some(task) = task {
                selected = Some((task, current_slot, round_end));
                break;
            }
            empty[priority as usize] = true;
        }
    }
    let Some((task, current_slot, round_end)) = selected else {
        return Ok(None);
    };
    transaction
        .execute(
            "UPDATE runtime_dispatch_state SET next_slot=?1 WHERE singleton=1",
            [((current_slot + 1) % WHEEL.len()) as i64],
        )
        .map_err(db)?;
    transaction.execute("UPDATE runtime_dispatch_classes SET cursor=?2,round_end=?3,selections=MIN(selections,9223372036854775806)+1 WHERE priority=?1",params![task.3,task.0 as i64,round_end]).map_err(db)?;
    transaction
        .execute(
            "INSERT INTO runtime_dispatch_visited VALUES(?1)",
            [task.0 as i64],
        )
        .map_err(db)?;
    transaction.commit().map_err(db)?;
    Ok(Some(task))
}

#[cfg(test)]
mod tests;

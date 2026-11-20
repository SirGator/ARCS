use arcs::core::VersionId;
use rusqlite::{Connection, Row, Transaction, params};

use super::{Journal, active, db, suspension::PAUSED};
use crate::HostError;
use crate::task_runtime::{
    config::invalid,
    queue::{TaskQueuePage, TaskQueueQuery, TaskQueueRow, TaskQueueScope, TaskWait},
};

impl Journal {
    pub(in crate::task_runtime) fn latest_task_sequence(
        connection: &Connection,
    ) -> Result<u64, HostError> {
        connection
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM runtime_jobs",
                [],
                |row| row.get::<_, i64>(0).map(|sequence| sequence as u64),
            )
            .map_err(db)
    }

    pub(in crate::task_runtime) fn task_queue(
        &self,
        query: TaskQueueQuery,
    ) -> Result<TaskQueuePage, HostError> {
        let tx = self.connection.unchecked_transaction().map_err(db)?;
        let last = Self::latest_task_sequence(&tx)?;
        let through = query.through_sequence.unwrap_or(last);
        if through > last || query.after_sequence > through {
            return Err(invalid("task queue sequence range is outside this journal"));
        }
        let mut ids = page_ids(&tx, &query, through)?;
        let more = ids.len() > query.limit;
        ids.truncate(query.limit);
        let next_after_sequence = if more {
            ids.last().map(|id| *id as u64)
        } else {
            None
        };
        let tasks = read_tasks(&tx, &ids)?;
        tx.commit().map_err(db)?;
        Ok(TaskQueuePage {
            scope: query.scope,
            after_sequence: query.after_sequence,
            through_sequence: through,
            next_after_sequence,
            tasks,
        })
    }
}

fn page_ids(
    tx: &Transaction<'_>,
    query: &TaskQueueQuery,
    through: u64,
) -> Result<Vec<i64>, HostError> {
    let after = query.after_sequence as i64;
    let through = through as i64;
    let limit = (query.limit + 1) as i64;
    if query.scope == TaskQueueScope::All {
        let mut statement = tx.prepare("SELECT sequence FROM runtime_jobs WHERE sequence>?1 AND sequence<=?2 ORDER BY sequence LIMIT ?3").map_err(db)?;
        return statement
            .query_map(params![after, through, limit], |row| row.get(0))
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db);
    }

    // Select small runs from the state/sequence index, then merge by sequence.
    // A sparse open queue does not require scanning the completed history.
    let open = active("j");
    let mut statement = tx.prepare(&format!("SELECT j.sequence FROM runtime_jobs j INDEXED BY runtime_pending
        WHERE j.state=?1 AND j.sequence>?2 AND j.sequence<=?3 AND {open} ORDER BY j.sequence LIMIT ?4")).map_err(db)?;
    let mut ids: Vec<i64> = Vec::new();
    let states = ["pending", "running", "awaiting_approval"];
    for state in states {
        let rows = statement
            .query_map(params![state, after, through, limit], |row| row.get(0))
            .map_err(db)?;
        for row in rows {
            ids.push(row.map_err(db)?);
        }
    }
    ids.sort_unstable();
    ids.truncate(limit as usize);
    Ok(ids)
}

fn read_tasks(tx: &Transaction<'_>, ids: &[i64]) -> Result<Vec<TaskQueueRow>, HostError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let open = active("j");
    let child_open = active("child");
    let sql = format!("SELECT j.sequence, j.route_id, j.input_version, priority.priority, j.state,
        json_extract(j.goal_json,'$.state'),
        CASE WHEN NOT ({open}) THEN NULL
             WHEN j.state='awaiting_approval' THEN 'approval'
             WHEN deps.state='waiting' THEN 'dependency'
             WHEN schedule.sequence IS NOT NULL AND schedule.not_before IS NULL THEN 'event'
             WHEN schedule.sequence IS NOT NULL THEN 'time' END,
        CASE WHEN {open} THEN schedule.not_before END,
        deadline.expires_at, parent.parent_sequence, occurrence.series,
        COALESCE(control.revision,0), COALESCE(control.paused,0), {PAUSED},
        (({open}) OR EXISTS(SELECT 1 FROM runtime_plan_steps s JOIN runtime_jobs child ON child.sequence=s.sequence
            WHERE s.parent_sequence=j.sequence AND {child_open})),
        EXISTS(SELECT 1 FROM runtime_jobs work
            WHERE (work.sequence=j.sequence OR work.sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=j.sequence))
              AND work.state='running' AND json_extract(work.goal_json,'$.state')='executing'
              AND json_extract(work.goal_json,'$.dispatch_cursor') IS NOT NULL)
        FROM runtime_jobs j
        JOIN runtime_job_priorities priority ON priority.sequence=j.sequence
        LEFT JOIN runtime_job_joins deps ON deps.sequence=j.sequence
        LEFT JOIN runtime_job_schedules schedule ON schedule.sequence=j.sequence AND schedule.released_at IS NULL
        LEFT JOIN runtime_task_deadlines deadline ON deadline.sequence=j.sequence
        LEFT JOIN runtime_plan_steps parent ON parent.sequence=j.sequence
        LEFT JOIN runtime_check_occurrences occurrence ON occurrence.sequence=j.sequence
        LEFT JOIN runtime_task_controls control ON control.sequence=j.sequence
        WHERE j.sequence IN (SELECT value FROM json_each(?1)) ORDER BY j.sequence");
    let selected = serde_json::to_string(ids).map_err(|error| invalid(error.to_string()))?;
    let mut statement = tx.prepare(&sql).map_err(db)?;
    let tasks = statement
        .query_map([selected], read_task)
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    if tasks.len() != ids.len() {
        return Err(invalid("task queue metadata is incomplete"));
    }
    Ok(tasks)
}

fn read_task(row: &Row<'_>) -> rusqlite::Result<TaskQueueRow> {
    let optional_sequence = |column| {
        row.get::<_, Option<i64>>(column)
            .map(|value| value.map(|n| n as u64))
    };
    let wait = match row.get::<_, Option<String>>(6)?.as_deref() {
        None => None,
        Some("time") => Some(TaskWait::Time),
        Some("event") => Some(TaskWait::Event),
        Some("dependency") => Some(TaskWait::Dependency),
        Some("approval") => Some(TaskWait::Approval),
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(TaskQueueRow {
        sequence: row.get::<_, i64>(0)? as u64,
        route_id: row.get(1)?,
        input_version_id: VersionId(row.get(2)?),
        priority: row.get(3)?,
        state: row.get(4)?,
        goal_state: row.get(5)?,
        waiting_for: wait,
        not_before_unix_ms: optional_sequence(7)?,
        deadline_unix_ms: optional_sequence(8)?,
        parent_plan_sequence: optional_sequence(9)?,
        series_sequence: optional_sequence(10)?,
        control_revision: row.get::<_, i64>(11)? as u64,
        locally_paused: row.get(12)?,
        paused: row.get(13)?,
        can_control: row.get(14)?,
        action_recovery_required: row.get(15)?,
    })
}

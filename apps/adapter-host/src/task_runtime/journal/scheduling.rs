//! Persistenz des Schedulers. Keine externen Aufrufe in Schreibtransaktionen.
mod schema;

use std::collections::HashSet;

use arcs::adapters::AdapterId;
use rusqlite::{OptionalExtension, Row, Transaction, params};

use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::config::invalid;
use crate::task_runtime::scheduler::{Activation, ScheduleState, ScheduledTask};
use arcs::core::VersionId;

pub(super) use schema::initialize;

const SELECT: &str = "SELECT j.sequence,j.route_id,j.input_version,s.activation_json,
    s.accepted_at,s.not_before,s.event_cursor,s.released_at,s.wake_event_version,s.revision
    FROM runtime_job_schedules s JOIN runtime_jobs j ON j.sequence=s.sequence";

struct SavedSchedule {
    sequence: u64,
    revision: u32,
    route_id: String,
    input: String,
    activation: String,
    accepted_at: u64,
    not_before: Option<u64>,
    cursor: u64,
    released_at: Option<u64>,
    wake: Option<String>,
}
fn read(row: &Row<'_>) -> rusqlite::Result<SavedSchedule> {
    Ok(SavedSchedule {
        sequence: row.get::<_, i64>(0)? as u64,
        revision: row.get(9)?,
        route_id: row.get(1)?,
        input: row.get(2)?,
        activation: row.get(3)?,
        accepted_at: row.get::<_, i64>(4)? as u64,
        not_before: row.get::<_, Option<i64>>(5)?.map(|n| n as u64),
        cursor: row.get::<_, i64>(6)? as u64,
        released_at: row.get::<_, Option<i64>>(7)?.map(|n| n as u64),
        wake: row.get(8)?,
    })
}
impl SavedSchedule {
    fn decode(self) -> Result<ScheduledTask, HostError> {
        let activation: Activation = serde_json::from_str(&self.activation)
            .map_err(|e| invalid(format!("invalid saved activation: {e}")))?;
        let waits_for_event = activation.event_source().is_some();
        let state = if self.released_at.is_some() {
            ScheduleState::Ready
        } else if waits_for_event {
            ScheduleState::WaitingEvent
        } else {
            ScheduleState::WaitingTime
        };
        Ok(ScheduledTask {
            sequence: self.sequence,
            revision: self.revision,
            route_id: self.route_id,
            input_version_id: VersionId(self.input),
            activation,
            state,
            accepted_at_unix_ms: self.accepted_at,
            not_before_unix_ms: self.not_before.filter(|_| !waits_for_event),
            fallback_at_unix_ms: self.not_before.filter(|_| waits_for_event),
            event_cursor: self.cursor,
            released_at_unix_ms: self.released_at,
            wake_event_version_id: self.wake.map(VersionId),
        })
    }
}

impl Journal {
    /// Runde und Wartebedingung committen gemeinsam; vorherige Aktivierungen
    /// bleiben unverändert als eigene Revisionen erhalten.
    pub(in crate::task_runtime) fn wait_goal(
        &mut self,
        task: &crate::task_runtime::goal::GoalTask,
        activation: &Activation,
        source_sequence: u64,
        now: u64,
    ) -> Result<(), HostError> {
        let checkpoint = serde_json::to_string(task).map_err(|e| invalid(e.to_string()))?;
        let transaction = self.connection.transaction().map_err(db)?;
        Self::schedule(
            &transaction,
            task.sequence,
            source_sequence,
            activation,
            now,
        )?;
        let changed = transaction.execute(
            "UPDATE runtime_jobs SET state='pending',goal_json=?2,error=NULL WHERE sequence=?1 AND state IN ('running','failed')",
            params![task.sequence as i64, checkpoint],
        ).map_err(db)?;
        if changed != 1 {
            return Err(invalid("goal cannot enter a wait from this queue state"));
        }
        transaction.commit().map_err(db)
    }

    pub(in crate::task_runtime) fn current_schedule(
        &self,
        sequence: u64,
    ) -> Result<Option<ScheduledTask>, HostError> {
        self.connection
            .query_row(
                &format!("{SELECT} WHERE j.sequence=?1 ORDER BY s.revision DESC LIMIT 1"),
                [sequence.min(i64::MAX as u64) as i64],
                read,
            )
            .optional()
            .map_err(db)?
            .map(SavedSchedule::decode)
            .transpose()
    }

    /// Wird in derselben Transaktion wie Aufgabe und Discovery-Cursor aufgerufen.
    pub(in crate::task_runtime) fn schedule(
        transaction: &Transaction<'_>,
        sequence: u64,
        source_sequence: u64,
        activation: &Activation,
        now: u64,
    ) -> Result<(), HostError> {
        let due = activation.due_at(now)?;
        let adapter = activation.event_source().map(|source| &source.adapter_id.0);
        let revision: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(revision),-1)+1 FROM runtime_job_schedules WHERE sequence=?1",
                [sequence as i64],
                |row| row.get(0),
            )
            .map_err(db)?;
        if revision > u32::MAX as i64 {
            return Err(invalid("schedule revision exhausted"));
        }
        transaction.execute("INSERT INTO runtime_job_schedules(sequence,activation_json,accepted_at,not_before,event_adapter,event_cursor,revision) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![sequence as i64,serde_json::to_string(activation).map_err(|e| invalid(e.to_string()))?,now as i64,due.map(|n| n as i64),adapter,source_sequence as i64,revision]).map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn release_timers(&self, now: u64) -> Result<(), HostError> {
        self.connection.execute("UPDATE runtime_job_schedules SET released_at=?1 WHERE released_at IS NULL AND not_before<=?1 AND EXISTS(SELECT 1 FROM runtime_jobs j WHERE j.sequence=runtime_job_schedules.sequence AND j.state='pending')",[now as i64]).map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn next_event_wait(
        &self,
    ) -> Result<Option<ScheduledTask>, HostError> {
        self.connection.query_row(&format!("{SELECT} WHERE s.released_at IS NULL AND s.event_adapter IS NOT NULL AND j.state='pending' ORDER BY (j.sequence<=(SELECT cursor FROM runtime_scheduler WHERE singleton=1)),j.sequence LIMIT 1"),[],read)
            .optional().map_err(db)?.map(SavedSchedule::decode).transpose()
    }

    pub(in crate::task_runtime) fn record_event_scan(
        &mut self,
        sequence: u64,
        cursor: u64,
        wake: Option<&VersionId>,
        now: u64,
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        transaction.execute("UPDATE runtime_job_schedules SET event_cursor=?2,released_at=?3,wake_event_version=?4 WHERE sequence=?1 AND released_at IS NULL",
            params![sequence as i64,cursor as i64,wake.map(|_| now as i64),wake.map(|v| &v.0)]).map_err(db)?;
        transaction
            .execute(
                "UPDATE runtime_scheduler SET cursor=?1 WHERE singleton=1",
                [sequence as i64],
            )
            .map_err(db)?;
        transaction.commit().map_err(db)
    }

    pub(in crate::task_runtime) fn waiting_input_adapters(
        &self,
    ) -> Result<HashSet<AdapterId>, HostError> {
        let mut query = self.connection.prepare("SELECT DISTINCT s.event_adapter FROM runtime_job_schedules s JOIN runtime_jobs j ON j.sequence=s.sequence WHERE s.released_at IS NULL AND s.event_adapter IS NOT NULL AND j.state='pending'").map_err(db)?;
        query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db)?
            .map(|row| row.map(AdapterId).map_err(db))
            .collect()
    }

    pub(in crate::task_runtime) fn schedule_summary(
        &self,
    ) -> Result<(u64, u64, Option<u64>), HostError> {
        self.connection.query_row("SELECT COUNT(CASE WHEN s.event_adapter IS NULL THEN 1 END),COUNT(CASE WHEN s.event_adapter IS NOT NULL THEN 1 END),MIN(s.not_before) FROM runtime_job_schedules s JOIN runtime_jobs j ON j.sequence=s.sequence WHERE s.released_at IS NULL AND j.state='pending'",[],|row|Ok((row.get::<_,i64>(0)? as u64,row.get::<_,i64>(1)? as u64,row.get::<_,Option<i64>>(2)?.map(|n| n as u64)))).map_err(db)
    }

    pub(in crate::task_runtime) fn scheduled_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<ScheduledTask>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("schedule page size must be 1..1000"));
        }
        let mut query = self
            .connection
            .prepare(&format!(
                "{SELECT} WHERE j.sequence>?1 AND s.revision=(SELECT MAX(latest.revision) FROM runtime_job_schedules latest WHERE latest.sequence=j.sequence) ORDER BY j.sequence LIMIT ?2"
            ))
            .map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                read,
            )
            .map_err(db)?;
        rows.map(|row| row.map_err(db).and_then(SavedSchedule::decode))
            .collect()
    }

    pub(in crate::task_runtime) fn scheduled_history(
        &self,
        sequence: u64,
        after_revision: Option<u32>,
        limit: usize,
    ) -> Result<Vec<ScheduledTask>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("schedule history page size must be 1..1000"));
        }
        let mut query = self
            .connection
            .prepare(&format!(
                "{SELECT} WHERE j.sequence=?1 AND s.revision>?2 ORDER BY s.revision LIMIT ?3"
            ))
            .map_err(db)?;
        let rows = query
            .query_map(
                params![
                    sequence.min(i64::MAX as u64) as i64,
                    after_revision.map_or(-1, i64::from),
                    limit as i64
                ],
                read,
            )
            .map_err(db)?;
        rows.map(|row| row.map_err(db).and_then(SavedSchedule::decode))
            .collect()
    }
}

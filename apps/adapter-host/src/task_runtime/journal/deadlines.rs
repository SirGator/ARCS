//! Lifetime admission, clock high-water mark and cancellation form one journal.
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::{Journal, db, runnable};
use crate::HostError;
use crate::task_runtime::{config::invalid, deadlines::TaskDeadline};
use arcs::core::VersionId;

const SELECT: &str = "SELECT j.sequence,j.route_id,j.input_version,d.accepted_at,d.expires_at,d.parent_sequence,d.expired_at,j.state FROM runtime_task_deadlines d JOIN runtime_jobs j ON j.sequence=d.sequence";

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_deadline_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1),last_unix_ms INTEGER NOT NULL CHECK(last_unix_ms>=0));
        INSERT OR IGNORE INTO runtime_deadline_clock VALUES(1,0);
        CREATE TABLE IF NOT EXISTS runtime_task_deadlines(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),accepted_at INTEGER NOT NULL CHECK(accepted_at>=0),expires_at INTEGER NOT NULL CHECK(expires_at>=accepted_at),parent_sequence INTEGER REFERENCES runtime_jobs(sequence),is_plan INTEGER NOT NULL CHECK(is_plan IN (0,1)),expired_at INTEGER CHECK(expired_at>=expires_at));
        CREATE INDEX IF NOT EXISTS runtime_deadline_due ON runtime_task_deadlines(expired_at,expires_at,sequence);
        COMMIT;").map_err(db)
}

fn eligible(include_failed: bool) -> String {
    format!(
        "({} OR (d.is_plan=1 AND j.state='blocked' AND EXISTS(SELECT 1 FROM runtime_plan_steps s JOIN runtime_jobs child ON child.sequence=s.sequence WHERE s.parent_sequence=j.sequence AND child.state IN ('pending','running','failed','awaiting_approval'))))",
        runnable("j", include_failed)
    )
}

fn read(row: &Row<'_>) -> rusqlite::Result<TaskDeadline> {
    Ok(TaskDeadline {
        sequence: row.get::<_, i64>(0)? as u64,
        route_id: row.get(1)?,
        input_version_id: VersionId(row.get(2)?),
        accepted_at_unix_ms: row.get::<_, i64>(3)? as u64,
        expires_at_unix_ms: row.get::<_, i64>(4)? as u64,
        inherited_from: row.get::<_, Option<i64>>(5)?.map(|n| n as u64),
        expired_at_unix_ms: row.get::<_, Option<i64>>(6)?.map(|n| n as u64),
        task_state: row.get(7)?,
    })
}

impl Journal {
    pub(in crate::task_runtime) fn advance_deadline_clock(
        &self,
        now: u64,
    ) -> Result<(), HostError> {
        self.connection.execute("UPDATE runtime_deadline_clock SET last_unix_ms=MAX(last_unix_ms,?1) WHERE singleton=1",[now as i64]).map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn assign_deadline(
        transaction: &Transaction<'_>,
        sequence: u64,
        lifetime: Option<u64>,
        is_plan: bool,
        parent: Option<u64>,
    ) -> Result<(), HostError> {
        let parent_expiry: Option<i64> = match parent {
            Some(parent) => transaction
                .query_row(
                    "SELECT expires_at FROM runtime_task_deadlines WHERE sequence=?1",
                    [parent as i64],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db)?,
            None => None,
        };
        if lifetime.is_none() && parent_expiry.is_none() {
            return Ok(());
        }
        let now: i64 = transaction
            .query_row(
                "SELECT last_unix_ms FROM runtime_deadline_clock WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        let own_expiry = lifetime
            .map(|ms| {
                (now as u64)
                    .checked_add(ms)
                    .filter(|n| *n <= i64::MAX as u64)
                    .ok_or_else(|| invalid("task lifetime deadline overflow"))
            })
            .transpose()?;
        let expiry = own_expiry
            .into_iter()
            .chain(parent_expiry.map(|n| n as u64))
            .min()
            .unwrap();
        if expiry < now as u64 {
            return Err(invalid("parent plan lifetime already elapsed"));
        }
        transaction.execute("INSERT INTO runtime_task_deadlines(sequence,accepted_at,expires_at,parent_sequence,is_plan) VALUES(?1,?2,?3,?4,?5)",params![sequence as i64,now,expiry as i64,parent.filter(|_|parent_expiry.is_some()).map(|n|n as i64),is_plan]).map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn expiring_tasks(
        &self,
        limit: usize,
        include_failed: bool,
    ) -> Result<Vec<u64>, HostError> {
        let mut query=self.connection.prepare(&format!("SELECT j.sequence FROM runtime_task_deadlines d JOIN runtime_jobs j ON j.sequence=d.sequence WHERE d.expired_at IS NULL AND d.expires_at<=(SELECT last_unix_ms FROM runtime_deadline_clock) AND {} ORDER BY d.expires_at,j.sequence LIMIT ?1",eligible(include_failed))).map_err(db)?;
        query
            .query_map([limit as i64], |row| row.get::<_, i64>(0).map(|n| n as u64))
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }

    pub(in crate::task_runtime) fn expire_task(
        &mut self,
        sequence: u64,
        include_failed: bool,
    ) -> Result<bool, HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        let is_plan:Option<bool>=transaction.query_row(&format!("SELECT d.is_plan FROM runtime_task_deadlines d JOIN runtime_jobs j ON j.sequence=d.sequence WHERE j.sequence=?1 AND d.expired_at IS NULL AND d.expires_at<=(SELECT last_unix_ms FROM runtime_deadline_clock) AND {}",eligible(include_failed)),[sequence as i64],|row|row.get(0)).optional().map_err(db)?;
        let Some(is_plan) = is_plan else {
            return Ok(false);
        };
        let now: i64 = transaction
            .query_row(
                "SELECT last_unix_ms FROM runtime_deadline_clock",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        let reason = format!("task lifetime exceeded: {sequence}");
        let affected = {
            let mut query=transaction.prepare("SELECT sequence FROM runtime_jobs WHERE sequence=?1 OR (sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1) AND state IN ('pending','running','failed','awaiting_approval'))").map_err(db)?;
            query
                .query_map([sequence as i64], |row| row.get::<_, i64>(0))
                .map_err(db)?
                .map(|row| row.map_err(db))
                .collect::<Result<Vec<_>, _>>()?
        };
        if is_plan {
            Self::cancel_plan_in(&transaction, sequence, &reason, now as u64)?;
        } else {
            transaction
                .execute(
                    "UPDATE runtime_jobs SET state='cancelled',error=?2 WHERE sequence=?1",
                    params![sequence as i64, reason],
                )
                .map_err(db)?;
            Self::cancel_goal_checkpoint(&transaction, sequence, &reason)?;
        }
        for task in affected {
            transaction.execute("UPDATE runtime_task_deadlines SET expired_at=?2 WHERE sequence=?1 AND expired_at IS NULL AND expires_at<=?2",params![task,now]).map_err(db)?;
            transaction.execute("UPDATE runtime_job_joins SET state='blocked',error=?2 WHERE sequence=?1 AND state='waiting'",params![task,reason]).map_err(db)?;
        }
        transaction.commit().map_err(db)?;
        Ok(true)
    }

    pub(in crate::task_runtime) fn deadline_summary(
        &self,
        include_failed: bool,
    ) -> Result<(u64, Option<u64>), HostError> {
        let expired = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM runtime_task_deadlines WHERE expired_at IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0).map(|n| n as u64),
            )
            .map_err(db)?;
        let next=self.connection.query_row(&format!("SELECT MIN(d.expires_at) FROM runtime_task_deadlines d JOIN runtime_jobs j ON j.sequence=d.sequence WHERE d.expired_at IS NULL AND {}",eligible(include_failed)),[],|row|row.get::<_,Option<i64>>(0).map(|n|n.map(|n|n as u64))).map_err(db)?;
        Ok((expired, next))
    }

    pub(in crate::task_runtime) fn deadlines_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<TaskDeadline>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("deadline page size must be 1..1000"));
        }
        let mut query = self
            .connection
            .prepare(&format!(
                "{SELECT} WHERE j.sequence>?1 ORDER BY j.sequence LIMIT ?2"
            ))
            .map_err(db)?;
        query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                read,
            )
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }

    pub(in crate::task_runtime) fn deadlines_for_report(
        &self,
    ) -> Result<Vec<TaskDeadline>, HostError> {
        let mut query = self.connection.prepare(&format!("{SELECT} ORDER BY (j.state IN ('pending','running','failed','awaiting_approval')) DESC,j.sequence DESC LIMIT 50")).map_err(db)?;
        query
            .query_map([], read)
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }
}

//! Ein atomarer Abbruchbeleg für Planner, Kindaufträge und wartende Joins.
use rusqlite::{OptionalExtension, Transaction, params};

use super::{Journal, VersionId, db, invalid};
use crate::HostError;

impl Journal {
    pub(in crate::task_runtime) fn planning_jobs_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<crate::task_runtime::planning::PlannerJob>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("planner page size must be 1..1000"));
        }
        let mut query=self.connection.prepare("SELECT j.sequence,j.route_id,j.input_version,j.state FROM runtime_job_reservations r JOIN runtime_jobs j ON j.sequence=r.sequence WHERE r.units>1 AND j.sequence>?1 AND j.state IN ('pending','running','failed') ORDER BY j.sequence LIMIT ?2").map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| {
                    Ok(crate::task_runtime::planning::PlannerJob {
                        sequence: row.get::<_, i64>(0)? as u64,
                        route_id: row.get(1)?,
                        input_version_id: VersionId(row.get(2)?),
                        state: row.get(3)?,
                    })
                },
            )
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }

    pub(in crate::task_runtime) fn plan_owner(&self, sequence: u64) -> Result<String, HostError> {
        self.connection.query_row("SELECT route_id FROM runtime_jobs WHERE sequence=?1 AND NOT EXISTS(SELECT 1 FROM runtime_plan_steps s WHERE s.sequence=runtime_jobs.sequence)",
            [sequence as i64],|row|row.get(0)).optional().map_err(db)?
            .ok_or_else(||invalid("unknown planner root task"))
    }

    pub(in crate::task_runtime) fn cancel_plan(
        &mut self,
        sequence: u64,
        reason: &str,
        now: u64,
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        Self::cancel_plan_in(&transaction, sequence, reason, now)?;
        transaction.commit().map_err(db)
    }

    pub(in crate::task_runtime::journal) fn cancel_plan_in(
        transaction: &Transaction<'_>,
        sequence: u64,
        reason: &str,
        now: u64,
    ) -> Result<(), HostError> {
        let previous: Option<String> = transaction
            .query_row(
                "SELECT reason FROM runtime_plan_cancellations WHERE sequence=?1",
                [sequence as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(previous) = previous {
            if previous != reason {
                return Err(invalid("plan cancellation reason cannot be changed"));
            }
        } else {
            let state: String = transaction
                .query_row(
                    "SELECT state FROM runtime_jobs WHERE sequence=?1",
                    [sequence as i64],
                    |row| row.get(0),
                )
                .map_err(db)?;
            let outstanding:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM runtime_plan_steps s JOIN runtime_jobs j ON j.sequence=s.sequence WHERE s.parent_sequence=?1 AND j.state IN ('pending','running','failed','awaiting_approval'))",[sequence as i64],|row|row.get(0)).map_err(db)?;
            if !["pending", "running", "failed"].contains(&state.as_str())
                && !(state == "blocked" && outstanding)
            {
                return Err(invalid("plan already ended"));
            }
            transaction
                .execute(
                    "INSERT INTO runtime_plan_cancellations VALUES(?1,?2,?3)",
                    params![sequence as i64, reason, now as i64],
                )
                .map_err(db)?;
            transaction.execute("UPDATE runtime_jobs SET state='cancelled',error=?2 WHERE sequence=?1 OR (sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1) AND state IN ('pending','running','failed','awaiting_approval'))",params![sequence as i64,reason]).map_err(db)?;
            transaction.execute("UPDATE runtime_job_joins SET state='blocked',error=?2 WHERE state='waiting' AND sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1)",params![sequence as i64,reason]).map_err(db)?;
            let checkpoints = {
                let mut query=transaction.prepare("SELECT sequence FROM runtime_jobs WHERE state='cancelled' AND goal_json IS NOT NULL AND sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1)").map_err(db)?;
                let rows = query
                    .query_map([sequence as i64], |row| row.get::<_, i64>(0))
                    .map_err(db)?;
                rows.map(|row| row.map_err(db))
                    .collect::<Result<Vec<_>, _>>()?
            };
            for child in checkpoints {
                Self::cancel_goal_checkpoint(transaction, child as u64, reason)?;
            }
        }
        Ok(())
    }

    pub(in crate::task_runtime::journal) fn cancel_goal_checkpoint(
        transaction: &Transaction<'_>,
        sequence: u64,
        reason: &str,
    ) -> Result<(), HostError> {
        let json: Option<String> = transaction
            .query_row(
                "SELECT goal_json FROM runtime_jobs WHERE sequence=?1",
                [sequence as i64],
                |row| row.get(0),
            )
            .map_err(db)?;
        if let Some(json) = json {
            let mut goal: crate::task_runtime::goal::GoalTask =
                serde_json::from_str(&json).map_err(|e| invalid(e.to_string()))?;
            if goal.sequence != sequence {
                return Err(invalid("goal checkpoint identity changed"));
            }
            goal.state = crate::task_runtime::goal::GoalState::Cancelled;
            goal.detail = reason.into();
            transaction
                .execute(
                    "UPDATE runtime_jobs SET goal_json=?2 WHERE sequence=?1",
                    params![
                        sequence as i64,
                        serde_json::to_string(&goal).map_err(|e| invalid(e.to_string()))?
                    ],
                )
                .map_err(db)?;
        }
        Ok(())
    }
}

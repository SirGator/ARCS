//! Revision-checked task control and checkpoint changes commit together.
use super::{Journal, active, db};
use crate::{
    HostError,
    task_runtime::{
        config::invalid,
        goal::{GoalState, GoalTask},
        suspension::*,
    },
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::BTreeMap;

// Plans currently have a single level of generated children (nested plans are
// rejected). This predicate automatically covers children admitted after pause.
pub(in crate::task_runtime) const PAUSED: &str = "(
    EXISTS(SELECT 1 FROM runtime_task_controls c WHERE c.paused=1 AND
        (c.sequence=j.sequence OR c.sequence=(SELECT parent_sequence FROM runtime_plan_steps WHERE sequence=j.sequence)))
    OR EXISTS(SELECT 1 FROM runtime_check_occurrences o JOIN runtime_check_series s ON s.sequence=o.series
        WHERE o.sequence=j.sequence AND s.control_state='paused'))";

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_task_controls(
        sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),revision INTEGER NOT NULL CHECK(revision>=1),
        paused INTEGER NOT NULL CHECK(paused IN (0,1)),reason TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS runtime_task_control_events(
        command_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),
        revision INTEGER NOT NULL,command_json TEXT NOT NULL,receipt_json TEXT NOT NULL,
        UNIQUE(sequence,revision));
        CREATE TABLE IF NOT EXISTS runtime_task_resume_barriers(
        sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),core_cursor INTEGER NOT NULL CHECK(core_cursor>=0));").map_err(db)?;
    let columns = {
        let mut q = connection
            .prepare("PRAGMA table_info(runtime_reasoning_requests)")
            .map_err(db)?;
        q.query_map([], |row| row.get::<_, String>(1))
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?
    };
    if !columns.iter().any(|column| column == "abandoned") {
        connection.execute("ALTER TABLE runtime_reasoning_requests ADD COLUMN abandoned INTEGER NOT NULL DEFAULT 0 CHECK(abandoned IN (0,1))",[]).map_err(db)?;
    }
    Ok(())
}

impl Journal {
    pub(in crate::task_runtime) fn task_control_history(
        &self,
        sequence: u64,
        after: u64,
        limit: usize,
    ) -> Result<Vec<TaskControlEvent>, HostError> {
        if sequence == 0 || sequence > i64::MAX as u64 || !(1..=1000).contains(&limit) {
            return Err(invalid("invalid task control history page"));
        }
        let mut query=self.connection.prepare("SELECT command_json,receipt_json FROM runtime_task_control_events WHERE sequence=?1 AND revision>?2 ORDER BY revision LIMIT ?3").map_err(db)?;
        query
            .query_map(
                params![
                    sequence as i64,
                    after.min(i64::MAX as u64) as i64,
                    limit as i64
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(db)?
            .map(|row| {
                let (command, receipt) = row.map_err(db)?;
                Ok(TaskControlEvent {
                    command: serde_json::from_str(&command).map_err(|e| invalid(e.to_string()))?,
                    receipt: serde_json::from_str(&receipt).map_err(|e| invalid(e.to_string()))?,
                })
            })
            .collect()
    }
    pub(in crate::task_runtime) fn task_is_paused(&self, sequence: u64) -> Result<bool, HostError> {
        self.connection
            .query_row(
                &format!("SELECT {PAUSED} FROM runtime_jobs j WHERE sequence=?1"),
                [sequence as i64],
                |row| row.get(0),
            )
            .map_err(db)
    }
    pub(in crate::task_runtime) fn control_scope(
        &self,
        sequence: u64,
    ) -> Result<Vec<(u64, String)>, HostError> {
        let mut q=self.connection.prepare("SELECT sequence,route_id FROM runtime_jobs WHERE sequence=?1 OR sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1) ORDER BY sequence").map_err(db)?;
        q.query_map([sequence as i64], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get(1)?))
        })
        .map_err(db)?
        .collect::<Result<_, _>>()
        .map_err(db)
    }
    pub(in crate::task_runtime) fn change_task_control(
        &mut self,
        command: &TaskControlCommand,
        now: u64,
        core_cursor: u64,
        goals: &BTreeMap<u64, u32>,
        is_plan: bool,
    ) -> Result<TaskControlReceipt, HostError> {
        let encoded = serde_json::to_string(command).map_err(|e| invalid(e.to_string()))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let previous:Option<(String,String)>=tx.query_row("SELECT command_json,receipt_json FROM runtime_task_control_events WHERE command_id=?1",[&command.command_id],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(db)?;
        if let Some((prior, receipt)) = previous {
            if prior != encoded {
                return Err(invalid(
                    "task control command ID was reused with different data",
                ));
            }
            return serde_json::from_str(&receipt).map_err(|e| invalid(e.to_string()));
        }
        let cancelled = matches!(command.change, TaskChange::Cancel);
        let active: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM runtime_jobs j
            WHERE (j.sequence=?1 OR j.sequence IN (SELECT sequence FROM runtime_plan_steps WHERE parent_sequence=?1)) AND {})", active("j")),
            [command.sequence as i64], |row| row.get(0)).map_err(db)?;
        if !active {
            return Err(invalid("task control requires unfinished work"));
        }
        let head: Option<(u64, bool)> = tx
            .query_row(
                "SELECT revision,paused FROM runtime_task_controls WHERE sequence=?1",
                [command.sequence as i64],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let (revision, paused) = head.unwrap_or((0, false));
        let next_paused = matches!(command.change, TaskChange::Pause);
        if revision != command.expected_revision {
            return Err(invalid("task control revision conflict"));
        }
        if !cancelled && paused == next_paused {
            return Err(invalid("task control already has this state"));
        }
        let revision = revision + 1;
        tx.execute("INSERT INTO runtime_task_controls VALUES(?1,?2,?3,?4) ON CONFLICT(sequence) DO UPDATE SET revision=excluded.revision,paused=excluded.paused,reason=excluded.reason",params![command.sequence as i64,revision as i64,next_paused,&command.reason]).map_err(db)?;
        if cancelled {
            if is_plan {
                Self::cancel_plan_in(&tx, command.sequence, &command.reason, now)?;
            } else {
                Self::cancel_task_in(&tx, command.sequence, &command.reason)?;
            }
        } else if !next_paused {
            let recover = matches!(
                command.change,
                TaskChange::Resume {
                    recover_dispatched: true
                }
            );
            for (sequence, max_rounds) in goals {
                let paused: bool = tx
                    .query_row(
                        &format!("SELECT {PAUSED} FROM runtime_jobs j WHERE sequence=?1"),
                        [*sequence as i64],
                        |row| row.get(0),
                    )
                    .map_err(db)?;
                if paused {
                    continue;
                }
                tx.execute("INSERT INTO runtime_task_resume_barriers VALUES(?1,?2) ON CONFLICT(sequence) DO UPDATE SET core_cursor=excluded.core_cursor",params![*sequence as i64,core_cursor as i64]).map_err(db)?;
                let raw:Option<String>=tx.query_row(&format!("SELECT goal_json FROM runtime_jobs j WHERE sequence=?1 AND NOT ({PAUSED})"),[*sequence as i64],|row|row.get(0)).optional().map_err(db)?.flatten();
                let Some(raw) = raw else {
                    continue;
                };
                let mut goal: GoalTask =
                    serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))?;
                if goal.state.terminal()
                    || matches!(goal.state, GoalState::WaitingTime | GoalState::WaitingEvent)
                {
                    continue;
                }
                // Planning may still carry the previous round's dispatch
                // references while its new planner request is already pending.
                // Only these phases are completing the current effect.
                if goal.dispatch_cursor.is_some()
                    && matches!(
                        goal.state,
                        GoalState::Executing | GoalState::Observing | GoalState::Assessing
                    )
                {
                    if goal.state == GoalState::Executing && !recover {
                        return Err(invalid(
                            "resuming an unresolved action dispatch requires recover_dispatched=true",
                        ));
                    }
                    // Complete the old effect/assessment before a new action.
                    // A subsequent planning round must acquire fresh data.
                    goal.resume_observation_cursor = Some(core_cursor);
                } else {
                    if goal.round >= *max_rounds {
                        goal.state = GoalState::BudgetExhausted;
                    } else {
                        goal.round += 1;
                        reset_goal(&mut goal, core_cursor);
                    }
                    // These requests belong to abandoned pre-action evidence.
                    tx.execute(
                        "UPDATE runtime_reasoning_requests SET abandoned=1 WHERE sequence=?1",
                        [*sequence as i64],
                    )
                    .map_err(db)?;
                }
                goal.detail = if goal.state == GoalState::BudgetExhausted {
                    "operator resumed task; no remaining goal rounds for fresh preparation"
                } else {
                    "operator resumed task; previous control decision is retained in the journal"
                }
                .into();
                tx.execute(
                    "UPDATE runtime_jobs SET state=?2,goal_json=?3,error=NULL WHERE sequence=?1",
                    params![
                        *sequence as i64,
                        goal.state.queue_state(),
                        serde_json::to_string(&goal).map_err(|e| invalid(e.to_string()))?
                    ],
                )
                .map_err(db)?;
            }
        }
        let receipt = TaskControlReceipt {
            sequence: command.sequence,
            revision,
            command_id: command.command_id.clone(),
            paused: next_paused,
            cancelled,
            changed_at_unix_ms: now,
        };
        tx.execute(
            "INSERT INTO runtime_task_control_events VALUES(?1,?2,?3,?4,?5)",
            params![
                command.command_id,
                command.sequence as i64,
                revision as i64,
                encoded,
                serde_json::to_string(&receipt).map_err(|e| invalid(e.to_string()))?
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(receipt)
    }

    // The caller checks eligibility inside the same transaction.
    pub(super) fn cancel_task_in(
        transaction: &Transaction<'_>,
        sequence: u64,
        reason: &str,
    ) -> Result<(), HostError> {
        transaction
            .execute(
                "UPDATE runtime_jobs SET state='cancelled',error=?2 WHERE sequence=?1",
                params![sequence as i64, reason],
            )
            .map_err(db)?;
        transaction.execute("UPDATE runtime_job_joins SET state='blocked',error=?2 WHERE sequence=?1 AND state='waiting'",params![sequence as i64,reason]).map_err(db)?;
        Self::cancel_goal_checkpoint(transaction, sequence, reason)
    }

    pub(in crate::task_runtime) fn resume_barrier(
        &self,
        sequence: u64,
    ) -> Result<Option<u64>, HostError> {
        self.connection
            .query_row(
                "SELECT core_cursor FROM runtime_task_resume_barriers WHERE sequence=?1",
                [sequence as i64],
                |row| row.get::<_, i64>(0).map(|n| n as u64),
            )
            .optional()
            .map_err(db)
    }

    pub(in crate::task_runtime) fn paused_reasoning(
        &self,
        requests: &[String],
    ) -> Result<Vec<(String, u64, String)>, HostError> {
        let mut query=self.connection.prepare(&format!("SELECT j.sequence,j.route_id FROM runtime_reasoning_requests r JOIN runtime_jobs j ON j.sequence=r.sequence WHERE r.request_id=?1 AND r.abandoned=0 AND {} AND {PAUSED}", active("j"))).map_err(db)?;
        let mut owned = Vec::new();
        for request in requests {
            if let Some((sequence, route)) = query
                .query_row([request], |row| {
                    Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
                })
                .optional()
                .map_err(db)?
            {
                owned.push((request.clone(), sequence, route));
            }
        }
        Ok(owned)
    }
    pub(in crate::task_runtime) fn paused_count(&self) -> Result<u64, HostError> {
        self.connection
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM runtime_jobs j WHERE {} AND {PAUSED}",
                    active("j")
                ),
                [],
                |row| row.get::<_, i64>(0).map(|n| n as u64),
            )
            .map_err(db)
    }
    pub(in crate::task_runtime) fn task_controls_after(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<TaskControlStatus>, HostError> {
        self.read_task_controls(after, limit, false)
    }
    pub(in crate::task_runtime) fn task_controls_report(
        &self,
    ) -> Result<Vec<TaskControlStatus>, HostError> {
        self.read_task_controls(0, 50, true)
    }
    fn read_task_controls(
        &self,
        after: u64,
        limit: usize,
        report: bool,
    ) -> Result<Vec<TaskControlStatus>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("task control page size must be 1..1000"));
        }
        let order = if report {
            format!("({}) DESC,j.sequence DESC", active("j"))
        } else {
            "j.sequence".into()
        };
        let mut q=self.connection.prepare(&format!("SELECT j.sequence,j.route_id,COALESCE(c.revision,0),COALESCE(c.paused,0),c.reason,COALESCE(json_extract(j.goal_json,'$.state')='executing' AND json_extract(j.goal_json,'$.dispatch_cursor') IS NOT NULL,0)
            FROM (SELECT sequence FROM runtime_task_controls UNION SELECT s.sequence FROM runtime_plan_steps s JOIN runtime_task_controls p ON p.sequence=s.parent_sequence) covered
            JOIN runtime_jobs j ON j.sequence=covered.sequence LEFT JOIN runtime_task_controls c ON c.sequence=j.sequence
            WHERE j.sequence>?1 ORDER BY {order} LIMIT ?2")).map_err(db)?;
        let rows = q
            .query_map(
                params![after.min(i64::MAX as u64) as i64, limit as i64],
                |row| {
                    Ok(TaskControlStatus {
                        sequence: row.get::<_, i64>(0)? as u64,
                        route_id: row.get(1)?,
                        revision: row.get::<_, i64>(2)? as u64,
                        paused: row.get(3)?,
                        reason: row.get(4)?,
                        action_recovery_required: row.get(5)?,
                        paused_by: vec![],
                    })
                },
            )
            .map_err(db)?;
        let mut result = Vec::new();
        for row in rows {
            let mut row = row.map_err(db)?;
            let mut parents=self.connection.prepare("SELECT sequence FROM runtime_task_controls WHERE paused=1 AND (sequence=?1 OR sequence=(SELECT parent_sequence FROM runtime_plan_steps WHERE sequence=?1)) ORDER BY sequence").map_err(db)?;
            row.paused_by = parents
                .query_map([row.sequence as i64], |row| {
                    row.get::<_, i64>(0).map(|n| n as u64)
                })
                .map_err(db)?
                .collect::<Result<_, _>>()
                .map_err(db)?;
            result.push(row);
        }
        Ok(result)
    }
}

pub(in crate::task_runtime) fn reset_goal(goal: &mut GoalTask, cursor: u64) {
    goal.state = GoalState::Acquiring;
    goal.observation_cursor = cursor;
    goal.wait_ticks = 0;
    goal.source_cursors.clear();
    goal.source_versions.clear();
    goal.before.clear();
    goal.after.clear();
    goal.candidate = None;
    goal.capability = None;
    goal.verification = None;
    goal.approval = None;
    goal.action = None;
    goal.execution = None;
    goal.assessment = None;
    goal.outcome = None;
    goal.dispatch_cursor = None;
    goal.resume_observation_cursor = None;
}

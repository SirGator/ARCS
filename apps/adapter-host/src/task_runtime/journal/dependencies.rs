//! Operative DAG-Bindung und eingefrorene Ergebnisreferenzen derselben Eingabe.
use arcs::core::VersionId;
use arcs::reasoning::ContextSelection;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::dependencies::{
    DependencyBinding, DependencyReceipt, DependencyState, ParentTask,
};
use crate::task_runtime::{ReasoningRoute, config::invalid};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_job_joins(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),state TEXT NOT NULL CHECK(state IN ('waiting','ready','blocked')),bindings_json TEXT NOT NULL DEFAULT '[]',error TEXT);
        CREATE INDEX IF NOT EXISTS runtime_dependency_waiting ON runtime_job_joins(state,sequence);
        CREATE TABLE IF NOT EXISTS runtime_job_dependencies(sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),parent_sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),ordinal INTEGER NOT NULL,payload_fields TEXT NOT NULL,PRIMARY KEY(sequence,parent_sequence),UNIQUE(sequence,ordinal),CHECK(sequence!=parent_sequence));
        CREATE TABLE IF NOT EXISTS runtime_dependency_scheduler(singleton INTEGER PRIMARY KEY CHECK(singleton=1),cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0));
        INSERT OR IGNORE INTO runtime_dependency_scheduler(singleton) VALUES(1);
        COMMIT;").map_err(db)
}

impl Journal {
    /// Alle zugehörigen Routenjobs existieren bereits in dieser Transaktion.
    pub(in crate::task_runtime) fn bind_dependencies(
        transaction: &Transaction<'_>,
        routes: &[&ReasoningRoute],
        input: &VersionId,
    ) -> Result<(), HostError> {
        for route in routes.iter().filter(|route| !route.dependencies.is_empty()) {
            let sequence: i64 = transaction
                .query_row(
                    "SELECT sequence FROM runtime_jobs WHERE route_id=?1 AND input_version=?2",
                    params![route.id, input.0],
                    |row| row.get(0),
                )
                .map_err(db)?;
            transaction
                .execute(
                    "INSERT INTO runtime_job_joins(sequence,state) VALUES(?1,'waiting')",
                    [sequence],
                )
                .map_err(db)?;
            for (ordinal, dependency) in route.dependencies.iter().enumerate() {
                let parent: i64 = transaction
                    .query_row(
                        "SELECT sequence FROM runtime_jobs WHERE route_id=?1 AND input_version=?2",
                        params![dependency.route_id, input.0],
                        |row| row.get(0),
                    )
                    .map_err(db)?;
                transaction.execute("INSERT INTO runtime_job_dependencies(sequence,parent_sequence,ordinal,payload_fields) VALUES(?1,?2,?3,?4)",params![sequence,parent,ordinal as i64,serde_json::to_string(&dependency.payload_fields).map_err(|e|invalid(e.to_string()))?]).map_err(db)?;
            }
        }
        Ok(())
    }

    pub(in crate::task_runtime) fn seed_previous_dependencies(
        transaction: &Transaction<'_>,
        sequence: u64,
        bindings: &[DependencyBinding],
    ) -> Result<(), HostError> {
        let encoded =
            serde_json::to_string(bindings).map_err(|error| invalid(error.to_string()))?;
        // A join with current-generation edges stays waiting. A step whose
        // inputs are all frozen is immediately eligible for the scheduler.
        let changed = transaction
            .execute(
                "INSERT INTO runtime_job_joins(sequence,state,bindings_json) VALUES(?1,'ready',?2)
             ON CONFLICT(sequence) DO UPDATE SET bindings_json=excluded.bindings_json
             WHERE runtime_job_joins.state='waiting'",
                params![sequence as i64, encoded],
            )
            .map_err(db)?;
        if changed != 1 {
            return Err(invalid("previous dependency exports could not be bound"));
        }
        Ok(())
    }

    pub(in crate::task_runtime) fn next_dependency_wait(
        &self,
    ) -> Result<Option<(u64, Vec<DependencyBinding>)>, HostError> {
        let row: Option<(i64, String)> = self.connection.query_row(
            "SELECT d.sequence,d.bindings_json FROM runtime_job_joins d
             JOIN runtime_jobs j ON j.sequence=d.sequence
             WHERE d.state='waiting' AND j.state='pending'
             ORDER BY (d.sequence<=(SELECT cursor FROM runtime_dependency_scheduler WHERE singleton=1)),d.sequence LIMIT 1",
            [],
            |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(db)?;
        row.map(|(sequence, bindings)| {
            let bindings = serde_json::from_str(&bindings).map_err(|error| {
                invalid(format!("invalid waiting dependency bindings: {error}"))
            })?;
            Ok((sequence as u64, bindings))
        })
        .transpose()
    }

    pub(in crate::task_runtime) fn dependency_parents(
        &self,
        sequence: u64,
    ) -> Result<Vec<ParentTask>, HostError> {
        let mut query = self.connection.prepare("SELECT p.sequence,p.route_id,p.input_version,p.state,p.result_json,p.goal_json,d.payload_fields FROM runtime_job_dependencies d JOIN runtime_jobs p ON p.sequence=d.parent_sequence WHERE d.sequence=?1 ORDER BY d.ordinal").map_err(db)?;
        let rows = query
            .query_map([sequence as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(db)?;
        rows.map(|row| {
            let (sequence, route_id, input, state, result, goal, fields) = row.map_err(db)?;
            Ok(ParentTask {
                sequence: sequence as u64,
                route_id,
                input: VersionId(input),
                state,
                result,
                goal,
                fields: serde_json::from_str(&fields)
                    .map_err(|e| invalid(format!("invalid dependency fields: {e}")))?,
            })
        })
        .collect()
    }

    pub(in crate::task_runtime) fn finish_dependency_scan(
        &mut self,
        sequence: u64,
        bindings: Option<&Vec<DependencyBinding>>,
        blocked: Option<&str>,
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        if let Some(reason) = blocked {
            transaction.execute("UPDATE runtime_job_joins SET state='blocked',error=?2 WHERE sequence=?1 AND state='waiting'",params![sequence as i64,reason]).map_err(db)?;
            transaction.execute("UPDATE runtime_jobs SET state='blocked',error=?2 WHERE sequence=?1 AND state='pending'",params![sequence as i64,reason]).map_err(db)?;
        } else if let Some(bindings) = bindings {
            transaction.execute("UPDATE runtime_job_joins SET state='ready',bindings_json=?2 WHERE sequence=?1 AND state='waiting'",params![sequence as i64,serde_json::to_string(bindings).map_err(|e|invalid(e.to_string()))?]).map_err(db)?;
        }
        transaction
            .execute(
                "UPDATE runtime_dependency_scheduler SET cursor=?1 WHERE singleton=1",
                [sequence as i64],
            )
            .map_err(db)?;
        transaction.commit().map_err(db)
    }

    pub(in crate::task_runtime) fn dependency_bindings(
        &self,
        sequence: u64,
    ) -> Result<Vec<DependencyBinding>, HostError> {
        let row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT state,bindings_json FROM runtime_job_joins WHERE sequence=?1",
                [sequence as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let Some((state, bindings)) = row else {
            return Ok(vec![]);
        };
        if state != "ready" {
            return Err(invalid("dependency results are not ready"));
        }
        let bindings: Vec<DependencyBinding> = serde_json::from_str(&bindings)
            .map_err(|e| invalid(format!("invalid dependency bindings: {e}")))?;
        Ok(bindings)
    }

    pub(in crate::task_runtime) fn dependency_context(
        &self,
        sequence: u64,
    ) -> Result<Vec<ContextSelection>, HostError> {
        Ok(self
            .dependency_bindings(sequence)?
            .into_iter()
            .flat_map(|binding| binding.context)
            .collect())
    }

    pub(in crate::task_runtime) fn dependency_summary(&self) -> Result<(u64, u64), HostError> {
        self.connection.query_row("SELECT COUNT(CASE WHEN d.state='waiting' AND j.state='pending' THEN 1 END),COUNT(CASE WHEN d.state='blocked' THEN 1 END) FROM runtime_job_joins d JOIN runtime_jobs j ON j.sequence=d.sequence",[],|row|Ok((row.get::<_,i64>(0)? as u64,row.get::<_,i64>(1)? as u64))).map_err(db)
    }

    pub(in crate::task_runtime) fn blocked_goal_count(&self) -> Result<u64, HostError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM runtime_jobs WHERE state IN ('blocked','budget_exhausted') AND goal_json IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0).map(|n| n as u64),
            )
            .map_err(db)
    }

    pub(in crate::task_runtime) fn dependencies_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<DependencyReceipt>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("dependency page size must be 1..1000"));
        }
        let mut query = self.connection.prepare("SELECT j.sequence,j.route_id,j.input_version,d.state,d.bindings_json,d.error FROM runtime_job_joins d JOIN runtime_jobs j ON j.sequence=d.sequence WHERE j.sequence>?1 ORDER BY j.sequence LIMIT ?2").map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .map_err(db)?;
        rows.map(|row| {
            let (sequence, route_id, input, state, bindings, detail) = row.map_err(db)?;
            let state = match state.as_str() {
                "waiting" => DependencyState::WaitingDependency,
                "ready" => DependencyState::Ready,
                "blocked" => DependencyState::Blocked,
                _ => return Err(invalid("invalid dependency state")),
            };
            Ok(DependencyReceipt {
                sequence: sequence as u64,
                route_id,
                input_version_id: VersionId(input),
                state,
                bindings: serde_json::from_str(&bindings)
                    .map_err(|e| invalid(format!("invalid dependency receipt: {e}")))?,
                detail,
            })
        })
        .collect()
    }
}

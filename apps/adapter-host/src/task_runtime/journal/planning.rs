//! Atomare Aufnahme begrenzter Teilpläne und reservierte Queuekapazität.
mod cancellation;
mod revisions;
use super::{Journal, active, db};
use crate::HostError;
use crate::task_runtime::planning::{AcceptedPlan, PlanReceipt, PlanState, PlanStepReport};
use crate::task_runtime::{CompletedTask, config::invalid};
use arcs::core::VersionId;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_job_reservations(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),units INTEGER NOT NULL CHECK(units BETWEEN 1 AND 32));
        CREATE TABLE IF NOT EXISTS runtime_plans(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),state TEXT NOT NULL CHECK(state IN ('accepted','ready','rejected','blocked')),proposal_version TEXT,plan_version TEXT,completion_json TEXT NOT NULL,error TEXT);
        CREATE TABLE IF NOT EXISTS runtime_plan_steps(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),parent_sequence INTEGER NOT NULL REFERENCES runtime_plans(sequence),node_id TEXT NOT NULL,template_id TEXT NOT NULL,UNIQUE(parent_sequence,node_id));
        CREATE INDEX IF NOT EXISTS runtime_plan_children ON runtime_plan_steps(parent_sequence,sequence);
        CREATE TABLE IF NOT EXISTS runtime_plan_cancellations(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),reason TEXT NOT NULL,cancelled_at INTEGER NOT NULL CHECK(cancelled_at>=0));
        COMMIT;").map_err(db)?;
    revisions::initialize(connection)
}

pub(in crate::task_runtime) fn capacity_query() -> String {
    format!(
        "SELECT COALESCE(SUM(COALESCE(r.units,1)),0) FROM runtime_jobs j LEFT JOIN runtime_job_reservations r ON r.sequence=j.sequence WHERE {} AND NOT EXISTS(SELECT 1 FROM runtime_plan_steps s JOIN runtime_jobs p ON p.sequence=s.parent_sequence JOIN runtime_job_reservations held ON held.sequence=p.sequence WHERE s.sequence=j.sequence AND held.units>1 AND {})",
        active("j"),
        active("p")
    )
}

pub(in crate::task_runtime) struct SavedPlan {
    pub state: String,
    pub plan_version: Option<VersionId>,
    pub completion: CompletedTask,
}

impl Journal {
    pub(in crate::task_runtime) fn capacity(&self) -> Result<usize, HostError> {
        self.connection
            .query_row(&capacity_query(), [], |row| {
                row.get::<_, i64>(0).map(|n| n as usize)
            })
            .map_err(db)
    }
    pub(in crate::task_runtime) fn reserve_plan(
        transaction: &Transaction<'_>,
        sequence: u64,
        units: usize,
    ) -> Result<(), HostError> {
        if units > 1 {
            transaction
                .execute(
                    "INSERT INTO runtime_job_reservations VALUES(?1,?2)",
                    params![sequence as i64, units as i64],
                )
                .map_err(db)?;
        }
        Ok(())
    }
    pub(in crate::task_runtime) fn saved_plan(
        &self,
        sequence: u64,
    ) -> Result<Option<SavedPlan>, HostError> {
        let raw: Option<(String, Option<String>, String)> = self
            .connection
            .query_row(
                "SELECT state,plan_version,completion_json FROM runtime_plans WHERE sequence=?1",
                [sequence as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(db)?;
        raw.map(|(state, version, completion)| {
            Ok(SavedPlan {
                state,
                plan_version: version.map(VersionId),
                completion: serde_json::from_str(&completion)
                    .map_err(|e| invalid(format!("invalid saved plan result: {e}")))?,
            })
        })
        .transpose()
    }
    pub(in crate::task_runtime) fn admit_plan(
        &mut self,
        plan: &AcceptedPlan,
        version: &VersionId,
        completion: &CompletedTask,
        now: u64,
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        let source: i64 = transaction
            .query_row(
                "SELECT source_sequence FROM runtime_jobs WHERE sequence=?1",
                [completion.sequence as i64],
                |row| row.get(0),
            )
            .map_err(db)?;
        let (reserved,input,route):(i64,String,String)=transaction.query_row(
            "SELECT COALESCE(r.units,1),j.input_version,j.route_id FROM runtime_jobs j LEFT JOIN runtime_job_reservations r ON r.sequence=j.sequence WHERE j.sequence=?1 AND j.state IN ('running','failed')",
            [completion.sequence as i64],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).map_err(db)?;
        if reserved < 1 + plan.steps.len() as i64
            || plan.parent_sequence != completion.sequence
            || input != plan.input_version_id.0
            || input != completion.input_version_id.0
            || route != plan.parent_route_id
            || route != completion.route_id
        {
            return Err(invalid(
                "plan admission lacks its reserved capacity or original task identity",
            ));
        }
        Self::record_plan_generation(&transaction, plan, version, completion)?;
        for step in &plan.steps {
            transaction.execute("INSERT INTO runtime_jobs(route_id,input_version,source_sequence,state) VALUES(?1,?2,?3,'pending')",params![step.route.id,plan.input_version_id.0,source]).map_err(db)?;
            let sequence = transaction.last_insert_rowid();
            Self::assign_priority(&transaction, sequence as u64, step.route.priority)?;
            Self::assign_deadline(
                &transaction,
                sequence as u64,
                step.route.max_lifetime_ms,
                false,
                Some(completion.sequence),
            )?;
            transaction
                .execute(
                    "INSERT INTO runtime_plan_steps VALUES(?1,?2,?3,?4)",
                    params![
                        sequence,
                        completion.sequence as i64,
                        if let Some(revision) = &plan.revision {
                            format!("revision:{}:{}", revision.index, step.id)
                        } else {
                            step.id.clone()
                        },
                        step.template_id
                    ],
                )
                .map_err(db)?;
            if let Some(revision) = &plan.revision {
                transaction
                    .execute(
                        "INSERT INTO runtime_plan_step_generations VALUES(?1,?2,?3,?4)",
                        params![sequence, revision.index, version.0, step.id],
                    )
                    .map_err(db)?;
            }
            if let Some(activation) = &step.route.activation {
                Self::schedule(
                    &transaction,
                    sequence as u64,
                    source as u64,
                    activation,
                    now,
                )?;
            }
        }
        let routes: Vec<_> = plan.steps.iter().map(|s| &s.route).collect();
        Self::bind_dependencies(&transaction, &routes, &plan.input_version_id)?;
        if plan.revision.is_none() {
            transaction
                .execute(
                    "UPDATE runtime_job_reservations SET units=1 WHERE sequence=?1",
                    [completion.sequence as i64],
                )
                .map_err(db)?;
        }
        transaction.commit().map_err(db)
    }
    pub(in crate::task_runtime) fn reject_plan(
        &mut self,
        completion: &CompletedTask,
        reason: &str,
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        transaction.execute("INSERT INTO runtime_plans(sequence,state,proposal_version,completion_json,error) VALUES(?1,'rejected',?2,?3,?4) ON CONFLICT(sequence) DO UPDATE SET state='rejected',proposal_version=excluded.proposal_version,completion_json=excluded.completion_json,error=excluded.error",params![completion.sequence as i64,completion.candidate_version_ids.first().map(|v|&v.0),serde_json::to_string(completion).map_err(|e|invalid(e.to_string()))?,reason]).map_err(db)?;
        transaction.execute("UPDATE runtime_plan_replanning SET state='rejected' WHERE parent_sequence=?1 AND state='planning'",[completion.sequence as i64]).map_err(db)?;
        transaction
            .execute(
                "UPDATE runtime_jobs SET state='blocked',error=?2 WHERE sequence=?1",
                params![completion.sequence as i64, reason],
            )
            .map_err(db)?;
        transaction.commit().map_err(db)
    }
    pub(in crate::task_runtime) fn finish_plan(
        &mut self,
        completion: &CompletedTask,
        blocked: Option<&str>,
        stop_children: bool,
        outputs: &[crate::task_runtime::plan_output::PlanOutputSeed],
    ) -> Result<(), HostError> {
        let transaction = self.connection.transaction().map_err(db)?;
        let json = serde_json::to_string(completion).map_err(|e| invalid(e.to_string()))?;
        if let Some(reason) = blocked {
            if stop_children {
                Self::cancel_plan_children_in(&transaction, completion.sequence, reason)?;
            }
            transaction
                .execute(
                    "UPDATE runtime_plans SET state='blocked',error=?2 WHERE sequence=?1",
                    params![completion.sequence as i64, reason],
                )
                .map_err(db)?;
            transaction
                .execute(
                    "UPDATE runtime_jobs SET state='blocked',error=?2 WHERE sequence=?1",
                    params![completion.sequence as i64, reason],
                )
                .map_err(db)?;
        } else {
            transaction
                .execute(
                    "UPDATE runtime_plans SET state='ready' WHERE sequence=?1",
                    [completion.sequence as i64],
                )
                .map_err(db)?;
            transaction.execute("UPDATE runtime_jobs SET state='ready_for_review',error=NULL,result_json=?2 WHERE sequence=?1",params![completion.sequence as i64,json]).map_err(db)?;
            Self::stage_plan_outputs(&transaction, completion.sequence, outputs)?;
        }
        transaction.commit().map_err(db)
    }
    pub(in crate::task_runtime) fn dynamic_origin(
        &self,
        sequence: u64,
    ) -> Result<Option<(u64, VersionId, String, VersionId)>, HostError> {
        self.connection.query_row("SELECT s.parent_sequence,COALESCE(g.plan_version,p.plan_version),COALESCE(g.node_id,s.node_id),j.input_version FROM runtime_plan_steps s JOIN runtime_plans p ON p.sequence=s.parent_sequence LEFT JOIN runtime_plan_step_generations g ON g.sequence=s.sequence JOIN runtime_jobs j ON j.sequence=s.sequence WHERE s.sequence=?1",[sequence as i64],|row|Ok((row.get::<_,i64>(0)? as u64,VersionId(row.get(1)?),row.get(2)?,VersionId(row.get(3)?)))).optional().map_err(db)
    }
    pub(in crate::task_runtime) fn plan_step_states(
        &self,
        parent: u64,
        version: &VersionId,
    ) -> Result<Vec<crate::task_runtime::dependencies::ParentTask>, HostError> {
        let mut query=self.connection.prepare("SELECT j.sequence,j.route_id,j.input_version,j.state,j.result_json,j.goal_json FROM runtime_plan_steps s JOIN runtime_jobs j ON j.sequence=s.sequence LEFT JOIN runtime_plan_step_generations g ON g.sequence=s.sequence JOIN runtime_plans p ON p.sequence=s.parent_sequence WHERE s.parent_sequence=?1 AND COALESCE(g.plan_version,p.plan_version)=?2 ORDER BY j.sequence").map_err(db)?;
        let rows = query
            .query_map(params![parent as i64, version.0], |row| {
                Ok(crate::task_runtime::dependencies::ParentTask {
                    sequence: row.get::<_, i64>(0)? as u64,
                    route_id: row.get(1)?,
                    input: VersionId(row.get(2)?),
                    state: row.get(3)?,
                    result: row.get(4)?,
                    goal: row.get(5)?,
                    fields: vec![],
                })
            })
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }
    pub(in crate::task_runtime) fn definition_version(&self) -> Result<VersionId, HostError> {
        let binding: String = self
            .connection
            .query_row("SELECT binding FROM runtime_core_binding", [], |row| {
                row.get(0)
            })
            .map_err(db)?;
        let value: serde_json::Value =
            serde_json::from_str(&binding).map_err(|e| invalid(e.to_string()))?;
        Ok(VersionId(
            value["anchor"]
                .as_str()
                .ok_or_else(|| invalid("missing Core definition anchor"))?
                .into(),
        ))
    }
    pub(in crate::task_runtime) fn plans_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<PlanReceipt>, HostError> {
        self.read_plans(sequence, limit, false)
    }
    pub(in crate::task_runtime) fn plan_summary(&self) -> Result<(u64, u64, u64), HostError> {
        self.connection.query_row("SELECT COUNT(CASE WHEN p.state='accepted' AND c.sequence IS NULL THEN 1 END),COUNT(CASE WHEN p.state IN ('blocked','rejected') OR c.sequence IS NOT NULL THEN 1 END),COUNT(CASE WHEN p.state='ready' THEN 1 END) FROM runtime_jobs j LEFT JOIN runtime_plans p ON p.sequence=j.sequence LEFT JOIN runtime_plan_cancellations c ON c.sequence=j.sequence WHERE p.sequence IS NOT NULL OR c.sequence IS NOT NULL",[],|row|Ok((row.get::<_,i64>(0)? as u64,row.get::<_,i64>(1)? as u64,row.get::<_,i64>(2)? as u64))).map_err(db)
    }
    pub(in crate::task_runtime) fn plans_for_report(&self) -> Result<Vec<PlanReceipt>, HostError> {
        self.read_plans(0, 50, true)
    }
    fn read_plans(
        &self,
        sequence: u64,
        limit: usize,
        for_report: bool,
    ) -> Result<Vec<PlanReceipt>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("plan page size must be 1..1000"));
        }
        let order = if for_report {
            "(p.state='accepted' AND c.sequence IS NULL) DESC,j.sequence DESC"
        } else {
            "j.sequence"
        };
        let mut query=self.connection.prepare(&format!("SELECT j.sequence,j.route_id,j.input_version,p.proposal_version,p.plan_version,CASE WHEN c.sequence IS NOT NULL THEN 'cancelled' WHEN EXISTS(SELECT 1 FROM runtime_plan_replanning intent WHERE intent.parent_sequence=j.sequence AND intent.state='planning') THEN 'replanning' ELSE p.state END,COALESCE(c.reason,p.error),c.cancelled_at FROM runtime_jobs j LEFT JOIN runtime_plans p ON p.sequence=j.sequence LEFT JOIN runtime_plan_cancellations c ON c.sequence=j.sequence WHERE j.sequence>?1 AND (p.sequence IS NOT NULL OR c.sequence IS NOT NULL) ORDER BY {order} LIMIT ?2")).map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .map_err(db)?;
        let mut result = Vec::new();
        for row in rows {
            let (sequence, route_id, input, proposal, version, state, detail, cancelled_at) =
                row.map_err(db)?;
            let mut children=self.connection.prepare("SELECT j.sequence,COALESCE(g.node_id,s.node_id),s.template_id,j.route_id,j.state,COALESCE(g.revision,0) FROM runtime_plan_steps s JOIN runtime_jobs j ON j.sequence=s.sequence LEFT JOIN runtime_plan_step_generations g ON g.sequence=s.sequence WHERE s.parent_sequence=?1 ORDER BY j.sequence").map_err(db)?;
            let steps = children
                .query_map([sequence], |row| {
                    Ok(PlanStepReport {
                        sequence: row.get::<_, i64>(0)? as u64,
                        id: row.get(1)?,
                        template_id: row.get(2)?,
                        route_id: row.get(3)?,
                        state: row.get(4)?,
                        revision: row.get(5)?,
                    })
                })
                .map_err(db)?
                .map(|row| row.map_err(db))
                .collect::<Result<Vec<_>, _>>()?;
            result.push(PlanReceipt {
                sequence: sequence as u64,
                route_id,
                input_version_id: VersionId(input),
                proposal_version_id: proposal.map(VersionId),
                plan_version_id: version.map(VersionId),
                state: match state.as_str() {
                    "accepted" => PlanState::Running,
                    "replanning" => PlanState::Replanning,
                    "ready" => PlanState::ReadyForReview,
                    "rejected" => PlanState::Rejected,
                    "blocked" => PlanState::Blocked,
                    "cancelled" => PlanState::Cancelled,
                    _ => return Err(invalid("invalid saved plan state")),
                },
                detail,
                cancelled_at_unix_ms: cancelled_at.map(|n| n as u64),
                steps,
                revisions: self.plan_revisions(sequence as u64)?,
            });
        }
        Ok(result)
    }
}

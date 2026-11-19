//! Generation pointers and immutable replanning checkpoints, not a second plan store.
use super::*;
use crate::task_runtime::planning::{PlanRevisionReceipt, ReplanningContext};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS runtime_plan_generations(
            parent_sequence INTEGER NOT NULL REFERENCES runtime_plans(sequence),revision INTEGER NOT NULL CHECK(revision BETWEEN 0 AND 7),
            plan_version TEXT NOT NULL UNIQUE,proposal_version TEXT NOT NULL,completion_json TEXT NOT NULL,context_version TEXT,
            PRIMARY KEY(parent_sequence,revision));
        CREATE TABLE IF NOT EXISTS runtime_plan_step_generations(
            sequence INTEGER PRIMARY KEY REFERENCES runtime_plan_steps(sequence),revision INTEGER NOT NULL CHECK(revision BETWEEN 0 AND 7),
            plan_version TEXT NOT NULL REFERENCES runtime_plan_generations(plan_version),node_id TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS runtime_plan_replanning(
            parent_sequence INTEGER NOT NULL REFERENCES runtime_plans(sequence),revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 7),
            context_json TEXT NOT NULL,context_version TEXT,state TEXT NOT NULL CHECK(state IN ('planning','accepted','rejected')),
            PRIMARY KEY(parent_sequence,revision));
        COMMIT;").map_err(db)
}

pub(in crate::task_runtime) struct SavedReplanning {
    pub context: ReplanningContext,
    pub version: Option<VersionId>,
}
impl Journal {
    pub(in crate::task_runtime) fn begin_replan(
        &mut self,
        context: &ReplanningContext,
    ) -> Result<(), HostError> {
        let encoded = serde_json::to_string(context).map_err(|e| invalid(e.to_string()))?;
        if encoded.len() > 60_000 {
            return Err(invalid("replanning checkpoint exceeds 60000 bytes"));
        }
        let tx = self.connection.transaction().map_err(db)?;
        let (version,input,state):(String,String,String)=tx.query_row("SELECT p.plan_version,j.input_version,j.state FROM runtime_plans p JOIN runtime_jobs j ON j.sequence=p.sequence WHERE p.sequence=?1 AND p.state='accepted'",[context.parent_sequence as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db)?;
        if version != context.previous_plan_version_id.0
            || input != context.input_version_id.0
            || !["pending", "running"].contains(&state.as_str())
        {
            return Err(invalid("replanning parent identity changed"));
        }
        let observed = {
            let mut q=tx.prepare("SELECT j.sequence,j.state FROM runtime_plan_steps s JOIN runtime_jobs j ON j.sequence=s.sequence JOIN runtime_plan_step_generations g ON g.sequence=s.sequence WHERE s.parent_sequence=?1 AND g.plan_version=?2 ORDER BY j.sequence").map_err(db)?;
            q.query_map(params![context.parent_sequence as i64, version], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
            })
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?
        };
        let expected: Vec<_> = context
            .steps
            .iter()
            .map(|s| (s.sequence, s.state.clone()))
            .collect();
        if observed != expected {
            return Err(invalid("replanning source states changed"));
        }
        tx.execute(
            "INSERT INTO runtime_plan_replanning VALUES(?1,?2,?3,NULL,'planning')",
            params![context.parent_sequence as i64, context.revision, encoded],
        )
        .map_err(db)?;
        Self::cancel_plan_children_in(
            &tx,
            context.parent_sequence,
            "superseded by a new plan revision",
        )?;
        tx.commit().map_err(db)
    }

    pub(in crate::task_runtime) fn pending_replan(
        &self,
        parent: u64,
    ) -> Result<Option<SavedReplanning>, HostError> {
        let row:Option<(String,Option<String>)>=self.connection.query_row("SELECT context_json,context_version FROM runtime_plan_replanning WHERE parent_sequence=?1 AND state='planning' ORDER BY revision DESC LIMIT 1",[parent as i64],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        row.map(|(raw, version)| {
            Ok(SavedReplanning {
                context: serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))?,
                version: version.map(VersionId),
            })
        })
        .transpose()
    }
    pub(in crate::task_runtime) fn bind_replan_context(
        &self,
        parent: u64,
        revision: u8,
        version: &VersionId,
    ) -> Result<(), HostError> {
        let changed=self.connection.execute("UPDATE runtime_plan_replanning SET context_version=?3 WHERE parent_sequence=?1 AND revision=?2 AND state='planning' AND (context_version IS NULL OR context_version=?3)",params![parent as i64,revision,version.0]).map_err(db)?;
        if changed != 1 {
            return Err(invalid("replanning context identity changed"));
        }
        Ok(())
    }

    pub(in crate::task_runtime::journal) fn record_plan_generation(
        tx: &Transaction<'_>,
        plan: &AcceptedPlan,
        version: &VersionId,
        completion: &CompletedTask,
    ) -> Result<(), HostError> {
        let encoded = serde_json::to_string(completion).map_err(|e| invalid(e.to_string()))?;
        match &plan.revision {
            Some(meta) if meta.index > 0 => {
                let (raw,context_version):(String,String)=tx.query_row("SELECT context_json,context_version FROM runtime_plan_replanning WHERE parent_sequence=?1 AND revision=?2 AND state='planning'",params![plan.parent_sequence as i64,meta.index],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
                let context: ReplanningContext =
                    serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))?;
                if meta.previous_plan_version_id.as_ref() != Some(&context.previous_plan_version_id)
                    || meta.context_version_id.as_ref().map(|v| v.0.as_str())
                        != Some(context_version.as_str())
                {
                    return Err(invalid(
                        "accepted revision differs from its frozen replanning checkpoint",
                    ));
                }
                let changed=tx.execute("UPDATE runtime_plans SET state='accepted',proposal_version=?2,plan_version=?3,completion_json=?4,error=NULL WHERE sequence=?1 AND state='accepted' AND plan_version=?5",params![plan.parent_sequence as i64,plan.proposal_version_id.0,version.0,encoded,context.previous_plan_version_id.0]).map_err(db)?;
                if changed != 1 {
                    return Err(invalid("plan revision predecessor changed"));
                }
                tx.execute("UPDATE runtime_plan_replanning SET state='accepted' WHERE parent_sequence=?1 AND revision=?2",params![plan.parent_sequence as i64,meta.index]).map_err(db)?;
            }
            meta => {
                if meta.as_ref().is_some_and(|m| {
                    m.previous_plan_version_id.is_some() || m.context_version_id.is_some()
                }) {
                    return Err(invalid("initial plan cannot have a predecessor"));
                }
                tx.execute("INSERT INTO runtime_plans(sequence,state,proposal_version,plan_version,completion_json) VALUES(?1,'accepted',?2,?3,?4)",params![completion.sequence as i64,plan.proposal_version_id.0,version.0,encoded]).map_err(db)?;
            }
        }
        if let Some(meta) = &plan.revision {
            tx.execute(
                "INSERT INTO runtime_plan_generations VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    plan.parent_sequence as i64,
                    meta.index,
                    version.0,
                    plan.proposal_version_id.0,
                    encoded,
                    meta.context_version_id.as_ref().map(|v| v.0.as_str())
                ],
            )
            .map_err(db)?;
        }
        Ok(())
    }
    pub(in crate::task_runtime) fn plan_revisions(
        &self,
        parent: u64,
    ) -> Result<Vec<PlanRevisionReceipt>, HostError> {
        let mut q=self.connection.prepare("SELECT revision,plan_version,proposal_version,context_version FROM runtime_plan_generations WHERE parent_sequence=?1 ORDER BY revision LIMIT 8").map_err(db)?;
        q.query_map([parent as i64], |r| {
            Ok(PlanRevisionReceipt {
                index: r.get(0)?,
                plan_version_id: VersionId(r.get(1)?),
                proposal_version_id: VersionId(r.get(2)?),
                context_version_id: r.get::<_, Option<String>>(3)?.map(VersionId),
            })
        })
        .map_err(db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db)
    }
    pub(in crate::task_runtime) fn plan_completion(
        &self,
        parent: u64,
        version: &VersionId,
    ) -> Result<CompletedTask, HostError> {
        let raw:String=self.connection.query_row("SELECT completion_json FROM runtime_plan_generations WHERE parent_sequence=?1 AND plan_version=?2 UNION ALL SELECT completion_json FROM runtime_plans WHERE sequence=?1 AND plan_version=?2 LIMIT 1",params![parent as i64,version.0],|r|r.get(0)).map_err(db)?;
        serde_json::from_str(&raw).map_err(|e| invalid(e.to_string()))
    }
    pub(in crate::task_runtime) fn planner_request_revision(
        &self,
        parent: u64,
        request: &VersionId,
    ) -> Result<u8, HostError> {
        self.connection.query_row("SELECT revision FROM runtime_plan_generations WHERE parent_sequence=?1 AND json_extract(completion_json,'$.request_version_id')=?2",params![parent as i64,request.0],|r|r.get(0)).optional().map_err(db).map(|r|r.unwrap_or(0))
    }
}

//! Session pointers are operational; ownership lives in Core artifacts.
use super::{Journal, db};
use crate::{HostError, task_runtime::config::invalid};
use arcs::{core::VersionId, runtime::reservations::ReservationOwner};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_control_sessions(sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),session INTEGER NOT NULL,owner_json TEXT NOT NULL,resources_json TEXT NOT NULL,closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)),PRIMARY KEY(sequence,session));
        CREATE UNIQUE INDEX IF NOT EXISTS runtime_control_open ON runtime_control_sessions(sequence) WHERE closed=0;").map_err(db)
}
pub(in crate::task_runtime) struct ControlSession {
    pub owner: ReservationOwner,
    pub resources: Vec<String>,
    pub closed: bool,
}
impl Journal {
    pub(in crate::task_runtime) fn control_session(
        &self,
        sequence: u64,
    ) -> Result<Option<ControlSession>, HostError> {
        let raw:Option<(String,String,bool)>=self.connection.query_row("SELECT owner_json,resources_json,closed FROM runtime_control_sessions WHERE sequence=?1 ORDER BY session DESC LIMIT 1",[sequence as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
        raw.map(|(owner, resources, closed)| {
            Ok(ControlSession {
                owner: serde_json::from_str(&owner).map_err(|e| invalid(e.to_string()))?,
                resources: serde_json::from_str(&resources).map_err(|e| invalid(e.to_string()))?,
                closed,
            })
        })
        .transpose()
    }
    pub(in crate::task_runtime) fn ensure_control_session(
        &self,
        sequence: u64,
        input: &VersionId,
        resources: &[String],
    ) -> Result<ControlSession, HostError> {
        let old = self.control_session(sequence)?;
        if let Some(old) = &old
            && !old.closed
        {
            if old.resources != resources || old.owner.input_version != *input {
                return Err(invalid("control session identity changed"));
            }
            return Ok(ControlSession {
                owner: old.owner.clone(),
                resources: old.resources.clone(),
                closed: false,
            });
        }
        let session = old
            .map_or(Some(1), |old| old.owner.session.checked_add(1))
            .ok_or_else(|| invalid("control session exhausted"))?;
        let owner = ReservationOwner {
            definition_version: self.definition_version()?,
            input_version: input.clone(),
            task_sequence: sequence,
            session,
        };
        self.connection.execute("INSERT INTO runtime_control_sessions(sequence,session,owner_json,resources_json) VALUES(?1,?2,?3,?4)",params![sequence as i64,session,serde_json::to_string(&owner).map_err(|e|invalid(e.to_string()))?,serde_json::to_string(resources).map_err(|e|invalid(e.to_string()))?]).map_err(db)?;
        Ok(ControlSession {
            owner,
            resources: resources.to_vec(),
            closed: false,
        })
    }
    pub(in crate::task_runtime) fn close_control_session(
        &self,
        owner: &ReservationOwner,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "UPDATE runtime_control_sessions SET closed=1 WHERE sequence=?1 AND session=?2",
                params![owner.task_sequence as i64, owner.session],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(in crate::task_runtime) fn control_cleanup_candidates(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<Vec<u64>, HostError> {
        let mut query=self.connection.prepare("SELECT c.sequence FROM runtime_control_sessions c JOIN runtime_jobs j ON j.sequence=c.sequence WHERE c.closed=0 AND (j.state!='pending' OR j.goal_json IS NOT NULL) AND j.state IN ('pending','completed','failed','blocked','rejected','budget_exhausted','cancelled') ORDER BY (c.sequence<=?1),c.sequence LIMIT ?2").map_err(db)?;
        let rows = query
            .query_map(params![cursor as i64, limit as i64], |row| {
                row.get::<_, i64>(0).map(|n| n as u64)
            })
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
    }
}

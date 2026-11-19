//! Session pointers are operational; ownership lives in Core artifacts.
use super::{Journal, db};
use crate::task_runtime::ControlReleaseIntent;
use crate::{HostError, task_runtime::config::invalid};
use arcs::{core::VersionId, runtime::reservations::ReservationOwner};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_control_sessions(sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),session INTEGER NOT NULL,owner_json TEXT NOT NULL,resources_json TEXT NOT NULL,closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)),PRIMARY KEY(sequence,session));
        CREATE UNIQUE INDEX IF NOT EXISTS runtime_control_open ON runtime_control_sessions(sequence) WHERE closed=0;").map_err(db)?;
    for column in ["closure_error", "release_intent_json"] {
        let has_column: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('runtime_control_sessions') WHERE name=?1)",
            [column], |row| row.get(0),
        ).map_err(db)?;
        if !has_column {
            connection
                .execute_batch(&format!(
                    "ALTER TABLE runtime_control_sessions ADD COLUMN {column} TEXT;"
                ))
                .map_err(db)?;
        }
    }
    Ok(())
}
pub(in crate::task_runtime) struct ControlSession {
    pub owner: ReservationOwner,
    pub resources: Vec<String>,
    pub closed: bool,
    pub release_intent: Option<ControlReleaseIntent>,
}
impl Journal {
    pub(in crate::task_runtime) fn control_session(
        &self,
        sequence: u64,
    ) -> Result<Option<ControlSession>, HostError> {
        let raw = self.connection.query_row("SELECT owner_json,resources_json,closed,release_intent_json FROM runtime_control_sessions WHERE sequence=?1 ORDER BY session DESC LIMIT 1",[sequence as i64],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get::<_,Option<String>>(3)?))).optional().map_err(db)?;
        raw.map(|(owner, resources, closed, release_intent)| {
            Ok(ControlSession {
                owner: serde_json::from_str(&owner).map_err(|e| invalid(e.to_string()))?,
                resources: serde_json::from_str(&resources).map_err(|e| invalid(e.to_string()))?,
                closed,
                release_intent: release_intent
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|e| invalid(e.to_string()))?,
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
                release_intent: old.release_intent.clone(),
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
            release_intent: None,
        })
    }
    pub(in crate::task_runtime) fn close_control_session(
        &self,
        owner: &ReservationOwner,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "UPDATE runtime_control_sessions SET closed=1,closure_error=NULL WHERE sequence=?1 AND session=?2",
                params![owner.task_sequence as i64, owner.session],
            )
            .map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn note_control_release_error(
        &self,
        owner: &ReservationOwner,
        error: &str,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "UPDATE runtime_control_sessions SET closure_error=?3
             WHERE sequence=?1 AND session=?2 AND closed=0 AND closure_error IS NOT ?3",
                params![owner.task_sequence as i64, owner.session, error],
            )
            .map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn save_control_release_intent(
        &self,
        owner: &ReservationOwner,
        intent: &ControlReleaseIntent,
    ) -> Result<(), HostError> {
        let intent = serde_json::to_string(intent).map_err(|error| invalid(error.to_string()))?;
        let updated = self
            .connection
            .execute(
                "UPDATE runtime_control_sessions SET release_intent_json=?3
             WHERE sequence=?1 AND session=?2 AND closed=0
             AND (release_intent_json IS NULL OR release_intent_json=?3)",
                params![owner.task_sequence as i64, owner.session, intent],
            )
            .map_err(db)?;
        if updated != 1 {
            return Err(invalid("control release decision changed"));
        }
        Ok(())
    }

    pub(in crate::task_runtime) fn control_release_waits(
        &self,
    ) -> Result<Vec<crate::task_runtime::ControlReleaseWait>, HostError> {
        let mut query = self
            .connection
            .prepare(
                "SELECT sequence,session,closure_error FROM runtime_control_sessions
             WHERE closed=0 AND closure_error IS NOT NULL ORDER BY sequence LIMIT 50",
            )
            .map_err(db)?;
        let rows = query
            .query_map([], |row| {
                Ok(crate::task_runtime::ControlReleaseWait {
                    sequence: row.get::<_, i64>(0)? as u64,
                    session: row.get(1)?,
                    error: row.get(2)?,
                })
            })
            .map_err(db)?;
        rows.map(|row| row.map_err(db)).collect()
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

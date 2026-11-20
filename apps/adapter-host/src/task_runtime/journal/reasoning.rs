//! Bind live Reason-Manager invocations to their owning operational task.
use super::{Journal, active, db};
use crate::{HostError, task_runtime::config::invalid};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_reasoning_requests(request_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence));
        CREATE INDEX IF NOT EXISTS runtime_task_reasoning ON runtime_reasoning_requests(sequence);
        CREATE TABLE IF NOT EXISTS runtime_recovery_bindings(sequence INTEGER PRIMARY KEY REFERENCES runtime_jobs(sequence),definition_version TEXT NOT NULL);").map_err(db)
}
impl Journal {
    pub(in crate::task_runtime) fn selection_job(
        &self,
        sequence: u64,
    ) -> Result<(String, arcs::core::VersionId), HostError> {
        self.connection
            .query_row(
                "SELECT route_id,input_version FROM runtime_jobs WHERE sequence=?1",
                [sequence.min(i64::MAX as u64) as i64],
                |row| Ok((row.get(0)?, arcs::core::VersionId(row.get(1)?))),
            )
            .map_err(db)
    }

    pub(in crate::task_runtime) fn selection_report_jobs(
        &self,
    ) -> Result<Vec<(u64, String)>, HostError> {
        let mut query = self.connection.prepare(&format!(
            "SELECT j.sequence,j.route_id FROM runtime_jobs j ORDER BY ({}) DESC,j.sequence DESC LIMIT 50",
            active("j")
        )).map_err(db)?;
        query
            .query_map([], |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)))
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }
    pub(in crate::task_runtime) fn bind_reasoning(
        &self,
        sequence: u64,
        request: &str,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO runtime_reasoning_requests(request_id,sequence) VALUES(?1,?2)",
                params![request, sequence as i64],
            )
            .map_err(db)?;
        let owner: i64 = self
            .connection
            .query_row(
                "SELECT sequence FROM runtime_reasoning_requests WHERE request_id=?1",
                [request],
                |row| row.get(0),
            )
            .map_err(db)?;
        if owner != sequence as i64 {
            return Err(invalid("reasoning request task identity changed"));
        }
        Ok(())
    }
    pub(in crate::task_runtime) fn cancelled_reasoning(
        &self,
        requests: &[String],
    ) -> Result<HashSet<String>, HostError> {
        // Only inspect live worker IDs (at most the bounded pool), rather than
        // loading all cancelled tasks or all historical goal rounds.
        let mut query=self.connection.prepare("SELECT 1 FROM runtime_reasoning_requests r JOIN runtime_jobs j ON j.sequence=r.sequence WHERE r.request_id=?1 AND (j.state='cancelled' OR r.abandoned=1)").map_err(db)?;
        let mut cancelled = HashSet::new();
        for request in requests {
            if query
                .query_row([request], |_| Ok(()))
                .optional()
                .map_err(db)?
                .is_some()
            {
                cancelled.insert(request.clone());
            }
        }
        Ok(cancelled)
    }
}

impl Journal {
    pub(in crate::task_runtime) fn bind_recovery(
        &self,
        sequence: u64,
        definition: &arcs::core::VersionId,
    ) -> Result<(), HostError> {
        self.connection.execute("INSERT INTO runtime_recovery_bindings(sequence,definition_version) VALUES(?1,?2) ON CONFLICT(sequence) DO UPDATE SET definition_version=excluded.definition_version WHERE definition_version<>excluded.definition_version",params![sequence as i64,definition.0]).map_err(db)?;
        Ok(())
    }
    pub(in crate::task_runtime) fn recovery_report_jobs(
        &self,
    ) -> Result<Vec<(u64, String, arcs::core::VersionId)>, HostError> {
        let mut statement=self.connection.prepare("SELECT b.sequence,j.route_id,b.definition_version FROM runtime_recovery_bindings b JOIN runtime_jobs j ON j.sequence=b.sequence ORDER BY b.sequence DESC LIMIT 50").map_err(db)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get(1)?,
                    arcs::core::VersionId(row.get(2)?),
                ))
            })
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }
}

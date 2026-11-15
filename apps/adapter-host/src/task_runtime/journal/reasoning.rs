//! Bind live Reason-Manager invocations to their owning operational task.
use super::{Journal, db};
use crate::{HostError, task_runtime::config::invalid};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS runtime_reasoning_requests(request_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence));
        CREATE INDEX IF NOT EXISTS runtime_task_reasoning ON runtime_reasoning_requests(sequence);").map_err(db)
}
impl Journal {
    pub(in crate::task_runtime) fn bind_reasoning(
        &self,
        sequence: u64,
        request: &str,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO runtime_reasoning_requests VALUES(?1,?2)",
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
        let mut query=self.connection.prepare("SELECT 1 FROM runtime_reasoning_requests r JOIN runtime_jobs j ON j.sequence=r.sequence WHERE r.request_id=?1 AND j.state='cancelled'").map_err(db)?;
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

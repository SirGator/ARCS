//! Dauerhafte Lebenszeitgrenzen; Abbruch verwendet die vorhandenen Task-Zustände.
use arcs::core::VersionId;
use serde::{Deserialize, Serialize};

use super::{TaskRuntime, TickReport};
use crate::{Host, HostError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDeadline {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub accepted_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub inherited_from: Option<u64>,
    pub expired_at_unix_ms: Option<u64>,
    pub task_state: String,
}

impl TaskRuntime {
    pub(super) fn expire_due_tasks(
        &mut self,
        host: &mut Host,
        report: &mut TickReport,
    ) -> Result<(), HostError> {
        let due = self.journal.expiring_tasks(self.config.max_jobs_per_tick)?;
        for sequence in due {
            self.expire_task(host, sequence, report)?;
        }
        Ok(())
    }

    pub(super) fn expire_task(
        &mut self,
        host: &mut Host,
        sequence: u64,
        report: &mut TickReport,
    ) -> Result<bool, HostError> {
        if !self.journal.expire_task(sequence)? {
            return Ok(false);
        }
        report.expired_tasks.push(sequence);
        // Only after the durable cancellation commits may a late worker result
        // be discarded. Its capacity remains held until the worker actually ends.
        self.reap_abandoned_task_work(host)?;
        self.cleanup_control_sessions(host)?;
        Ok(true)
    }

    pub fn deadlines_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<TaskDeadline>, HostError> {
        self.journal.deadlines_after(sequence, limit)
    }
}

//! Planabbruch über dieselbe Runtime: keine zweite Steuerungsinstanz.
use crate::task_runtime::{TaskRuntime, config::invalid, scheduler::now_unix_ms};
use crate::{Host, HostError};

impl TaskRuntime {
    /// Offene Planner vor dem ersten angenommenen/abgewiesenen Vorschlag.
    pub fn planning_jobs_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<super::PlannerJob>, HostError> {
        self.journal.planning_jobs_after(sequence, limit)
    }

    /// Stoppt die noch offene Arbeit einer Planinstanz einschließlich Planner.
    /// Fertige Ergebnisse und der unveränderliche Core-Plan bleiben erhalten.
    /// Gleiche Wiederholungen sind idempotent; neue Begründungen überschreiben
    /// keinen vorhandenen Abbruchbeleg.
    pub fn cancel_plan(
        &mut self,
        host: &mut Host,
        sequence: u64,
        reason: String,
    ) -> Result<(), HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        if sequence == 0
            || sequence > i64::MAX as u64
            || reason.trim().is_empty()
            || reason.len() > 2000
        {
            return Err(invalid("invalid plan cancellation identity or reason"));
        }
        let route = self.journal.plan_owner(sequence)?;
        if !self
            .config
            .routes
            .iter()
            .any(|r| r.id == route && r.plan.is_some())
        {
            return Err(invalid("cancellation requires a planner root task"));
        }
        // Commit first. If the transaction fails, no live worker is discarded.
        self.journal
            .cancel_plan(sequence, &reason, now_unix_ms()?)?;
        self.cleanup_control_sessions(host)?;
        self.reap_abandoned_task_work(host)
    }
}

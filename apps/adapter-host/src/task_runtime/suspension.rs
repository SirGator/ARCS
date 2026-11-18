//! Durable scheduling control inside the existing task runtime.
//! Operators authenticate at the application boundary; models gain no control port.
use super::{TaskRuntime, config::invalid, scheduler::now_unix_ms};
use crate::{Host, HostError};
use arcs::core::{Actor, ActorType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskChange {
    Pause,
    Resume {
        #[serde(default)]
        recover_dispatched: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskControlCommand {
    pub sequence: u64,
    pub expected_revision: u64,
    pub command_id: String,
    pub operator: Actor,
    pub reason: String,
    pub change: TaskChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskControlReceipt {
    pub sequence: u64,
    pub revision: u64,
    pub command_id: String,
    pub paused: bool,
    pub changed_at_unix_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct TaskControlEvent {
    pub command: TaskControlCommand,
    pub receipt: TaskControlReceipt,
}

#[derive(Debug, Serialize)]
pub struct TaskControlStatus {
    pub sequence: u64,
    pub route_id: String,
    pub revision: u64,
    pub paused: bool,
    pub paused_by: Vec<u64>,
    pub reason: Option<String>,
    pub action_recovery_required: bool,
}

impl TaskControlCommand {
    pub fn validate(&self) -> Result<(), HostError> {
        if self.sequence == 0
            || self.sequence > i64::MAX as u64
            || self.expected_revision >= i64::MAX as u64
            || self.command_id.is_empty()
            || self.command_id.len() > 128
            || !self
                .command_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
            || self.reason.trim().is_empty()
            || self.reason.len() > 2000
            || self.reason.chars().any(char::is_control)
            || self.operator.id.trim().is_empty()
            || self.operator.id.len() > 128
            || self.operator.id.chars().any(char::is_control)
            || !matches!(
                self.operator.actor_type,
                ActorType::Human | ActorType::System
            )
        {
            return Err(invalid(
                "invalid task control identity, actor, revision or reason",
            ));
        }
        Ok(())
    }
}

impl TaskRuntime {
    pub fn control_task(
        &mut self,
        host: &mut Host,
        command: TaskControlCommand,
    ) -> Result<TaskControlReceipt, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        command.validate()?;
        // Resolve every possible child only from the bound operator definition.
        let affected = self.journal.control_scope(command.sequence)?;
        let mut limits = std::collections::BTreeMap::new();
        for (sequence, route) in affected {
            if let Some(goal) = self.raw_route_for_job(host, sequence, &route)?.goal {
                limits.insert(sequence, goal.max_rounds);
            }
        }
        let receipt = self.journal.change_task_control(
            &command,
            now_unix_ms()?,
            host.store().latest_sequence().map_err(HostError::Store)?,
            &limits,
        )?;
        // A failed control transaction must never discard a live response.
        self.reap_cancelled_task_reasoning(host)?;
        for sequence in limits.keys() {
            self.cleanup_control(host, *sequence)?;
        }
        Ok(receipt)
    }

    pub fn task_controls_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<TaskControlStatus>, HostError> {
        self.journal.task_controls_after(sequence, limit)
    }

    pub fn task_control_history(
        &self,
        sequence: u64,
        after_revision: u64,
        limit: usize,
    ) -> Result<Vec<TaskControlEvent>, HostError> {
        self.journal
            .task_control_history(sequence, after_revision, limit)
    }

    pub(super) fn collect_paused_reasoning(
        &mut self,
        host: &mut Host,
        has_deadlines: bool,
        live_clock: bool,
        report: &mut super::TickReport,
    ) -> Result<(), HostError> {
        let owned = self
            .journal
            .paused_reasoning(&host.pending_reasoning_request_ids())?;
        for (id, sequence, route) in owned {
            if has_deadlines && live_clock {
                self.journal.advance_deadline_clock(now_unix_ms()?)?;
            }
            if has_deadlines && self.expire_task(host, sequence, report)? {
                continue;
            }
            let requests = std::collections::HashSet::from([id]);
            for (_, result) in host.collect_reasoning_requests(&requests) {
                if let Err(error) = result {
                    let route = self.raw_route_for_job(host, sequence, &route)?;
                    self.record_job_failure(sequence, &route, error, report)?;
                }
            }
        }
        Ok(())
    }
}

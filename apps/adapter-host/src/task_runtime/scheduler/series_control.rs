use arcs::core::Actor;
use serde::{Deserialize, Serialize};

use super::now_unix_ms;
use crate::task_runtime::{TaskRuntime, config::invalid, suspension::valid_control_fields};
use crate::{Host, HostError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesState {
    #[default]
    Active,
    Paused,
    Cancelled,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesChange {
    Pause,
    Resume,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesControlCommand {
    pub sequence: u64,
    pub expected_revision: u64,
    pub command_id: String,
    pub operator: Actor,
    pub reason: String,
    pub change: SeriesChange,
}

impl SeriesControlCommand {
    pub fn validate(&self) -> Result<(), HostError> {
        if !valid_control_fields(
            self.sequence,
            self.expected_revision,
            &self.command_id,
            &self.operator,
            &self.reason,
        ) {
            return Err(invalid(
                "invalid series control identity, actor, revision or reason",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesControlReceipt {
    pub sequence: u64,
    pub revision: u64,
    pub command_id: String,
    pub state: SeriesState,
    pub changed_at_unix_ms: u64,
    pub cancelled_occurrence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesControlEvent {
    pub command: SeriesControlCommand,
    pub receipt: SeriesControlReceipt,
}

impl TaskRuntime {
    pub fn control_check_series(
        &mut self,
        host: &mut Host,
        command: SeriesControlCommand,
    ) -> Result<SeriesControlReceipt, HostError> {
        command.validate()?;
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        let receipt = self.journal.change_series_control(
            &command,
            now_unix_ms()?,
            self.config.retry_failed_jobs(),
        )?;
        if receipt.cancelled_occurrence.is_some() {
            self.reap_abandoned_task_work(host)?;
        }
        Ok(receipt)
    }

    pub fn series_control_history(
        &self,
        sequence: u64,
        after_revision: u64,
        limit: usize,
    ) -> Result<Vec<SeriesControlEvent>, HostError> {
        self.journal
            .series_control_history(sequence, after_revision, limit)
    }
}

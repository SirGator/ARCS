//! Durable scheduling of separately verified and approved Output deliveries.
use arcs::adapters::AdapterId;
use arcs::core::VersionId;
use serde::{Deserialize, Serialize};

use super::{TaskRuntime, config::invalid, is_core_failure};
use crate::{Host, HostError};

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputCommand {
    Enqueue {
        adapter_id: AdapterId,
        request_version_id: VersionId,
        approval_version_id: VersionId,
    },
    Inspect {
        sequence: u64,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputState {
    Queued,
    Delivered,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct OutputDelivery {
    pub sequence: u64,
    pub invocation_id: String,
    pub adapter_id: AdapterId,
    pub request_version_id: VersionId,
    pub approval_version_id: VersionId,
    pub receipt_version_id: Option<VersionId>,
    pub state: OutputState,
    pub error: Option<String>,
}

impl TaskRuntime {
    pub fn control_output(
        &self,
        host: &mut Host,
        command: OutputCommand,
    ) -> Result<OutputDelivery, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        match command {
            OutputCommand::Enqueue {
                adapter_id,
                request_version_id,
                approval_version_id,
            } => {
                let invocation = host.check_output_delivery(
                    &adapter_id,
                    &request_version_id,
                    &approval_version_id,
                )?;
                self.journal.enqueue_output(&invocation)
            }
            OutputCommand::Inspect { sequence } if (1..=i64::MAX as u64).contains(&sequence) => {
                self.journal.output_delivery(sequence)
            }
            OutputCommand::Inspect { .. } => Err(invalid("invalid output sequence")),
        }
    }

    pub(super) fn advance_output_deliveries(&self, host: &mut Host) -> Result<(), HostError> {
        for job in self.journal.pending_output_heads()? {
            let outcome = host.poll_output_delivery(
                &job.adapter_id,
                &job.request_version_id,
                &job.approval_version_id,
                &job.invocation_id,
            );
            match outcome {
                Ok(Some(receipt)) => self
                    .journal
                    .finish_output(job.sequence, Ok(&receipt.version_id))?,
                Ok(None) | Err(HostError::AdapterBusy(_) | HostError::ResourceBusy { .. }) => {}
                Err(error) if is_core_failure(&error) => return Err(error),
                Err(error) => self.journal.finish_output(job.sequence, Err(&error))?,
            }
        }
        Ok(())
    }
}

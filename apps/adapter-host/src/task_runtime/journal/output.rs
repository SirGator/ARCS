//! Operational Output scheduling. Approval and delivery evidence live in the Core.
use arcs::adapters::AdapterId;
use arcs::core::VersionId;
use arcs::output::OutputInvocation;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{
    config::invalid,
    output::{OutputDelivery, OutputState},
};

const MAX_QUEUED_OUTPUTS: i64 = 128;
const COLUMNS: &str =
    "sequence,invocation_id,adapter,request_version,approval_version,receipt_version,state,error";

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS runtime_output_deliveries (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            invocation_id TEXT NOT NULL UNIQUE,
            adapter TEXT NOT NULL,
            request_version TEXT NOT NULL,
            approval_version TEXT NOT NULL,
            receipt_version TEXT,
            state TEXT NOT NULL DEFAULT 'queued',
            error TEXT,
            CHECK((state='queued' AND receipt_version IS NULL AND error IS NULL) OR
                (state='delivered' AND receipt_version IS NOT NULL AND error IS NULL) OR
                (state='failed' AND receipt_version IS NULL AND error IS NOT NULL))
        );
        CREATE INDEX IF NOT EXISTS runtime_output_queued
            ON runtime_output_deliveries(adapter,sequence) WHERE state='queued';",
        )
        .map_err(db)
}

impl Journal {
    pub(in crate::task_runtime) fn enqueue_output(
        &self,
        invocation: &OutputInvocation,
    ) -> Result<OutputDelivery, HostError> {
        // The runtime journal has one locked owner. Keep admission and its
        // idempotency check in the same short transaction.
        let tx = self.connection.unchecked_transaction().map_err(db)?;
        let existing = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM runtime_output_deliveries WHERE invocation_id=?1"),
                [&invocation.invocation_id],
                read,
            )
            .optional()
            .map_err(db)?;
        if let Some(existing) = existing {
            if existing.adapter_id != invocation.capability.adapter_id
                || existing.request_version_id != invocation.request_version_id
                || existing.approval_version_id != invocation.approval_version_id
            {
                return Err(invalid("saved output delivery identity changed"));
            }
            return Ok(existing);
        }
        let queued: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM runtime_output_deliveries WHERE state='queued'",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        if queued >= MAX_QUEUED_OUTPUTS {
            return Err(invalid("output queue is full"));
        }
        tx.execute(
            "INSERT INTO runtime_output_deliveries(invocation_id,adapter,request_version,approval_version)
             VALUES(?1,?2,?3,?4)",
            params![
                invocation.invocation_id,
                invocation.capability.adapter_id.0,
                invocation.request_version_id.0,
                invocation.approval_version_id.0,
            ],
        ).map_err(db)?;
        let sequence = tx.last_insert_rowid() as u64;
        tx.commit().map_err(db)?;
        self.output_delivery(sequence)
    }

    pub(in crate::task_runtime) fn output_delivery(
        &self,
        sequence: u64,
    ) -> Result<OutputDelivery, HostError> {
        self.connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM runtime_output_deliveries WHERE sequence=?1"),
                [sequence as i64],
                read,
            )
            .optional()
            .map_err(db)?
            .ok_or_else(|| invalid("unknown output delivery"))
    }

    pub(in crate::task_runtime) fn pending_output_heads(
        &self,
    ) -> Result<Vec<OutputDelivery>, HostError> {
        // A blocked adapter must not hide independent pending deliveries.
        self.output_deliveries_matching("WHERE sequence IN (
            SELECT MIN(sequence) FROM runtime_output_deliveries WHERE state='queued' GROUP BY adapter
        ) ORDER BY sequence")
    }

    pub(in crate::task_runtime) fn recent_output_deliveries(
        &self,
    ) -> Result<Vec<OutputDelivery>, HostError> {
        self.output_deliveries_matching("ORDER BY sequence DESC LIMIT 50")
    }

    fn output_deliveries_matching(&self, clause: &str) -> Result<Vec<OutputDelivery>, HostError> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM runtime_output_deliveries {clause}"
            ))
            .map_err(db)?;
        statement
            .query_map([], read)
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)
    }

    pub(in crate::task_runtime) fn finish_output(
        &self,
        sequence: u64,
        outcome: Result<&VersionId, &HostError>,
    ) -> Result<(), HostError> {
        let (state, receipt, error) = match outcome {
            Ok(receipt) => ("delivered", Some(&receipt.0), None),
            Err(error) => (
                "failed",
                None,
                Some(error.to_string().chars().take(2000).collect::<String>()),
            ),
        };
        let changed = self
            .connection
            .execute(
                "UPDATE runtime_output_deliveries SET state=?2,receipt_version=?3,error=?4
             WHERE sequence=?1 AND state='queued'",
                params![sequence as i64, state, receipt, error],
            )
            .map_err(db)?;
        if changed != 1 {
            return Err(invalid("output checkpoint changed"));
        }
        Ok(())
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<OutputDelivery> {
    let state = match row.get_ref(6)?.as_str()? {
        "queued" => OutputState::Queued,
        "delivered" => OutputState::Delivered,
        "failed" => OutputState::Failed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(OutputDelivery {
        sequence: row.get::<_, i64>(0)? as u64,
        invocation_id: row.get(1)?,
        adapter_id: AdapterId(row.get(2)?),
        request_version_id: VersionId(row.get(3)?),
        approval_version_id: VersionId(row.get(4)?),
        receipt_version_id: row.get::<_, Option<String>>(5)?.map(VersionId),
        state,
        error: row.get(7)?,
    })
}

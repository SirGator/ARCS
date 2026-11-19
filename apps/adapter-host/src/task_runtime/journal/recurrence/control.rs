//! Series controls and occurrence cancellation share one journal transaction.
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Journal, active, db, read_series, series_select, validate_page};
use crate::HostError;
use crate::task_runtime::{
    config::invalid,
    scheduler::{
        SeriesChange, SeriesControlCommand, SeriesControlEvent, SeriesControlReceipt, SeriesState,
    },
};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate).map_err(db)?;
    let upgraded: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('runtime_check_series') WHERE name='control_state')",
        [], |row| row.get(0),
    ).map_err(db)?;
    if !upgraded {
        tx.execute_batch("ALTER TABLE runtime_check_series ADD COLUMN control_revision INTEGER NOT NULL DEFAULT 0 CHECK(control_revision>=0);
            ALTER TABLE runtime_check_series ADD COLUMN control_state TEXT NOT NULL DEFAULT 'active' CHECK(control_state IN ('active','paused','cancelled'));
            ALTER TABLE runtime_check_series ADD COLUMN control_reason TEXT;").map_err(db)?;
    }
    tx.execute_batch("DROP INDEX IF EXISTS runtime_check_due;
        CREATE INDEX IF NOT EXISTS runtime_check_active_due ON runtime_check_series(next_due,sequence)
            WHERE control_state='active' AND next_due IS NOT NULL;
        CREATE TABLE IF NOT EXISTS runtime_check_control_events(
            command_id TEXT PRIMARY KEY,
            sequence INTEGER NOT NULL REFERENCES runtime_check_series(sequence),
            revision INTEGER NOT NULL CHECK(revision>=1),
            command_json TEXT NOT NULL,
            receipt_json TEXT NOT NULL,
            UNIQUE(sequence,revision));").map_err(db)?;
    tx.commit().map_err(db)
}

fn encode(value: &impl Serialize) -> Result<String, HostError> {
    serde_json::to_string(value).map_err(|error| invalid(error.to_string()))
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, HostError> {
    serde_json::from_str(value).map_err(|error| invalid(error.to_string()))
}

impl Journal {
    pub(in crate::task_runtime) fn change_series_control(
        &mut self,
        command: &SeriesControlCommand,
        now: u64,
        include_failed: bool,
    ) -> Result<SeriesControlReceipt, HostError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let previous: Option<(String, String)> = tx.query_row(
            "SELECT command_json,receipt_json FROM runtime_check_control_events WHERE command_id=?1",
            [&command.command_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(db)?;
        if let Some((prior, receipt)) = previous {
            if decode::<SeriesControlCommand>(&prior)? != *command {
                return Err(invalid(
                    "series control command ID was reused with different data",
                ));
            }
            return decode(&receipt);
        }
        let series = tx
            .query_row(
                &format!("{} WHERE s.sequence=?1", series_select(include_failed)),
                [command.sequence as i64],
                read_series,
            )
            .optional()
            .map_err(db)?
            .ok_or_else(|| invalid("unknown check series"))?;
        if command.expected_revision != series.control_revision {
            return Err(invalid("series control revision conflict"));
        }
        let (state, state_text) = match (series.state, command.change) {
            (SeriesState::Active, SeriesChange::Pause) => (SeriesState::Paused, "paused"),
            (SeriesState::Paused, SeriesChange::Resume) => (SeriesState::Active, "active"),
            (SeriesState::Active | SeriesState::Paused, SeriesChange::Cancel) => {
                (SeriesState::Cancelled, "cancelled")
            }
            _ => return Err(invalid("series control transition is not allowed")),
        };
        let revision = series.control_revision + 1;
        tx.execute(
            "UPDATE runtime_check_series SET control_revision=?2,control_state=?3,control_reason=?4 WHERE sequence=?1",
            params![series.sequence as i64, revision as i64, state_text, command.reason],
        ).map_err(db)?;

        let active_occurrence = active(include_failed);
        let mut cancelled_occurrence = None;
        match command.change {
            SeriesChange::Pause => {
                tx.execute(&format!("UPDATE runtime_reasoning_requests SET pause_captured=1
                    WHERE sequence=(SELECT sequence FROM runtime_jobs WHERE sequence=?1 AND {active_occurrence})"),
                    [series.last_sequence as i64],
                ).map_err(db)?;
            }
            SeriesChange::Resume => {}
            SeriesChange::Cancel => {
                let changed = tx
                    .execute(
                        &format!(
                            "UPDATE runtime_jobs SET state='cancelled',error=?2
                    WHERE sequence=?1 AND {active_occurrence}"
                        ),
                        params![series.last_sequence as i64, command.reason],
                    )
                    .map_err(db)?;
                cancelled_occurrence = (changed == 1).then_some(series.last_sequence);
            }
        }
        let receipt = SeriesControlReceipt {
            sequence: series.sequence,
            revision,
            command_id: command.command_id.clone(),
            state,
            changed_at_unix_ms: now,
            cancelled_occurrence,
        };
        tx.execute(
            "INSERT INTO runtime_check_control_events VALUES(?1,?2,?3,?4,?5)",
            params![
                command.command_id,
                series.sequence as i64,
                revision as i64,
                encode(command)?,
                encode(&receipt)?
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(receipt)
    }

    pub(in crate::task_runtime) fn series_control_history(
        &self,
        sequence: u64,
        after_revision: u64,
        limit: usize,
    ) -> Result<Vec<SeriesControlEvent>, HostError> {
        validate_page(limit)?;
        if !(1..=i64::MAX as u64).contains(&sequence) {
            return Err(invalid("invalid series sequence"));
        }
        let mut query = self
            .connection
            .prepare(
                "SELECT command_json,receipt_json FROM runtime_check_control_events
             WHERE sequence=?1 AND revision>?2 ORDER BY revision LIMIT ?3",
            )
            .map_err(db)?;
        let rows = query
            .query_map(
                params![
                    sequence as i64,
                    after_revision.min(i64::MAX as u64) as i64,
                    limit as i64
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(db)?;
        rows.map(|row| {
            let (command, receipt) = row.map_err(db)?;
            Ok(SeriesControlEvent {
                command: decode(&command)?,
                receipt: decode(&receipt)?,
            })
        })
        .collect()
    }
}

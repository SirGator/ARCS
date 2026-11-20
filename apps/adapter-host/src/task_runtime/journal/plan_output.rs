//! Operational drafts bind a completed plan generation to the existing Output review.
use arcs::core::VersionId;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{
    config::invalid,
    plan_output::{PlanOutputDraft, PlanOutputSeed, PlanOutputState},
};

const MAX_PENDING_PLAN_OUTPUTS: usize = 128;

const COLUMNS: &str =
    "sequence,parent_sequence,spec,state,request_version,verification_version,error";

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_plan_outputs (
            sequence INTEGER PRIMARY KEY,
            parent_sequence INTEGER NOT NULL REFERENCES runtime_plans(sequence),
            step_sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),
            spec TEXT NOT NULL,
            state TEXT NOT NULL,
            request_version TEXT,
            verification_version TEXT,
            error TEXT,
            UNIQUE(parent_sequence,step_sequence),
            CHECK((state='preparing' AND request_version IS NULL AND verification_version IS NULL AND error IS NULL)
                OR (state='ready' AND request_version IS NOT NULL AND verification_version IS NOT NULL AND error IS NULL)
                OR (state='failed' AND request_version IS NULL AND verification_version IS NULL AND error IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS runtime_plan_output_pending
            ON runtime_plan_outputs(sequence) WHERE state='preparing';",
    ).map_err(db)
}

impl Journal {
    pub(in crate::task_runtime) fn has_plan_output_capacity(
        &self,
        outputs: &[PlanOutputSeed],
    ) -> Result<bool, HostError> {
        let new = outputs
            .iter()
            .filter(|output| output.spec.result_version_id.is_some())
            .count();
        if new == 0 {
            return Ok(true);
        }
        let pending: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM runtime_plan_outputs WHERE state='preparing'",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        Ok(pending as usize + new <= MAX_PENDING_PLAN_OUTPUTS)
    }

    pub(super) fn stage_plan_outputs(
        transaction: &Transaction<'_>,
        parent_sequence: u64,
        outputs: &[PlanOutputSeed],
    ) -> Result<(), HostError> {
        for output in outputs {
            let spec = serde_json::to_string(output).map_err(|e| invalid(e.to_string()))?;
            let (state, error) = if output.spec.result_version_id.is_some() {
                ("preparing", None)
            } else {
                (
                    "failed",
                    Some("configured output result index was not returned by the step"),
                )
            };
            transaction.execute(
                "INSERT INTO runtime_plan_outputs(parent_sequence,step_sequence,spec,state,error) VALUES(?1,?2,?3,?4,?5)",
                params![parent_sequence as i64, output.spec.task_sequence as i64, spec, state, error],
            ).map_err(db)?;
        }
        Ok(())
    }

    pub(in crate::task_runtime) fn plan_outputs(
        &self,
        parent_sequence: u64,
    ) -> Result<Vec<PlanOutputDraft>, HostError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM runtime_plan_outputs WHERE parent_sequence=?1 ORDER BY sequence"
        )).map_err(db)?;
        statement
            .query_map([parent_sequence as i64], read)
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)
    }

    pub(in crate::task_runtime) fn plan_output(
        &self,
        sequence: u64,
    ) -> Result<PlanOutputDraft, HostError> {
        if !(1..=i64::MAX as u64).contains(&sequence) {
            return Err(invalid("invalid plan output sequence"));
        }
        self.connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM runtime_plan_outputs WHERE sequence=?1"),
                [sequence as i64],
                read,
            )
            .optional()
            .map_err(db)?
            .ok_or_else(|| invalid("unknown plan output"))
    }

    pub(in crate::task_runtime) fn pending_plan_outputs(
        &self,
    ) -> Result<Vec<PlanOutputDraft>, HostError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM runtime_plan_outputs WHERE state='preparing' ORDER BY sequence LIMIT 4"
        )).map_err(db)?;
        statement
            .query_map([], read)
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)
    }

    pub(in crate::task_runtime) fn finish_plan_output(
        &self,
        sequence: u64,
        outcome: Result<(&VersionId, &VersionId), &HostError>,
    ) -> Result<(), HostError> {
        let (state, request, verification, error) = match outcome {
            Ok((request, verification)) => ("ready", Some(&request.0), Some(&verification.0), None),
            Err(error) => (
                "failed",
                None,
                None,
                Some(error.to_string().chars().take(2000).collect::<String>()),
            ),
        };
        let changed = self.connection.execute(
            "UPDATE runtime_plan_outputs SET state=?2,request_version=?3,verification_version=?4,error=?5
             WHERE sequence=?1 AND state='preparing'",
            params![sequence as i64, state, request, verification, error],
        ).map_err(db)?;
        if changed != 1 {
            return Err(invalid("plan output checkpoint changed"));
        }
        Ok(())
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<PlanOutputDraft> {
    let state = match row.get_ref(3)?.as_str()? {
        "preparing" => PlanOutputState::Preparing,
        "ready" => PlanOutputState::Ready,
        "failed" => PlanOutputState::Failed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let spec: String = row.get(2)?;
    let PlanOutputSeed {
        spec,
        export_fields,
    } = serde_json::from_str(&spec).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(PlanOutputDraft {
        sequence: row.get::<_, i64>(0)? as u64,
        parent_sequence: row.get::<_, i64>(1)? as u64,
        spec,
        export_fields,
        state,
        request_version_id: row.get::<_, Option<String>>(4)?.map(VersionId),
        verification_version_id: row.get::<_, Option<String>>(5)?.map(VersionId),
        error: row.get(6)?,
    })
}

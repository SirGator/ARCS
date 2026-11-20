//! Persist the operator's decision before creating the Core approval and delivery.
use arcs::core::VersionId;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{
    config::invalid,
    dependencies::ParentTask,
    task_output::{TaskOutputDecision, TaskOutputReview, TaskOutputReviewState},
};

const COLUMNS: &str =
    "request_version,decision_json,state,approval_version,delivery_sequence,error";

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_task_output_reviews (
            sequence INTEGER PRIMARY KEY,
            request_version TEXT NOT NULL UNIQUE,
            decision_json TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'pending',
            approval_version TEXT,
            delivery_sequence INTEGER REFERENCES runtime_output_deliveries(sequence),
            error TEXT,
            CHECK((state='pending' AND approval_version IS NULL AND delivery_sequence IS NULL AND error IS NULL)
                OR (state='queued' AND approval_version IS NOT NULL AND delivery_sequence IS NOT NULL AND error IS NULL)
                OR (state='rejected' AND approval_version IS NOT NULL AND delivery_sequence IS NULL AND error IS NULL)
                OR (state='failed' AND approval_version IS NULL AND delivery_sequence IS NULL AND error IS NOT NULL))
        );
        CREATE INDEX IF NOT EXISTS runtime_task_output_pending
            ON runtime_task_output_reviews(sequence) WHERE state='pending';"
    ).map_err(db)
}

impl Journal {
    pub(in crate::task_runtime) fn task_output_source(
        &self,
        sequence: u64,
    ) -> Result<ParentTask, HostError> {
        self.connection.query_row(
            "SELECT route_id,input_version,state,result_json,goal_json FROM runtime_jobs WHERE sequence=?1",
            [sequence as i64],
            |row| Ok(ParentTask {
                sequence,
                route_id: row.get(0)?,
                input: VersionId(row.get(1)?),
                state: row.get(2)?,
                result: row.get(3)?,
                goal: row.get(4)?,
                fields: vec![],
            }),
        ).optional().map_err(db)?.ok_or_else(|| invalid("unknown output source task"))
    }

    pub(in crate::task_runtime) fn task_output_review(
        &self,
        request: &VersionId,
    ) -> Result<Option<(TaskOutputDecision, TaskOutputReview)>, HostError> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM runtime_task_output_reviews WHERE request_version=?1"
                ),
                [&request.0],
                read_review,
            )
            .optional()
            .map_err(db)
    }

    pub(in crate::task_runtime) fn stage_task_output(
        &self,
        decision: &TaskOutputDecision,
    ) -> Result<TaskOutputReview, HostError> {
        let pending: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM runtime_task_output_reviews WHERE state='pending'",
                [],
                |row| row.get(0),
            )
            .map_err(db)?;
        if pending >= 128 {
            return Err(invalid("task output review queue is full"));
        }
        let encoded = serde_json::to_string(decision).map_err(|e| invalid(e.to_string()))?;
        self.connection.execute(
            "INSERT INTO runtime_task_output_reviews(request_version,decision_json) VALUES(?1,?2)",
            params![&decision.request_version_id.0, encoded],
        ).map_err(db)?;
        self.task_output_review(&decision.request_version_id)?
            .map(|(_, review)| review)
            .ok_or_else(|| invalid("output review was not saved"))
    }

    pub(in crate::task_runtime) fn pending_task_output_reviews(
        &self,
    ) -> Result<Vec<TaskOutputDecision>, HostError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM runtime_task_output_reviews WHERE state='pending' ORDER BY sequence LIMIT 8"
        )).map_err(db)?;
        statement
            .query_map([], read_review)
            .map_err(db)?
            .map(|row| row.map(|(decision, _)| decision).map_err(db))
            .collect()
    }

    pub(in crate::task_runtime) fn finish_task_output_review(
        &self,
        request: &VersionId,
        outcome: Result<(&VersionId, Option<u64>), &HostError>,
    ) -> Result<(), HostError> {
        let (state, approval, delivery, error) = match outcome {
            Ok((approval, Some(delivery))) => {
                ("queued", Some(&approval.0), Some(delivery as i64), None)
            }
            Ok((approval, None)) => ("rejected", Some(&approval.0), None, None),
            Err(error) => (
                "failed",
                None,
                None,
                Some(error.to_string().chars().take(2000).collect::<String>()),
            ),
        };
        let changed = self.connection.execute(
            "UPDATE runtime_task_output_reviews SET state=?2,approval_version=?3,delivery_sequence=?4,error=?5
             WHERE request_version=?1 AND state='pending'",
            params![&request.0, state, approval, delivery, error],
        ).map_err(db)?;
        if changed != 1 {
            return Err(invalid("output review checkpoint changed"));
        }
        Ok(())
    }
}

fn read_review(row: &Row<'_>) -> rusqlite::Result<(TaskOutputDecision, TaskOutputReview)> {
    let request_version_id = VersionId(row.get(0)?);
    let raw: String = row.get(1)?;
    let decision: TaskOutputDecision = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    if decision.request_version_id != request_version_id {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let state = match row.get_ref(2)?.as_str()? {
        "pending" => TaskOutputReviewState::Pending,
        "queued" => TaskOutputReviewState::Queued,
        "rejected" => TaskOutputReviewState::Rejected,
        "failed" => TaskOutputReviewState::Failed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok((
        decision,
        TaskOutputReview {
            request_version_id,
            state,
            approval_version_id: row.get::<_, Option<String>>(3)?.map(VersionId),
            delivery_sequence: row.get::<_, Option<i64>>(4)?.map(|n| n as u64),
            error: row.get(5)?,
        },
    ))
}

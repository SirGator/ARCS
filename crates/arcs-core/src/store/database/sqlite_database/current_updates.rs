//! Bounded optimistic updates to current artifacts, with atomic provenance links.
use super::*;
use crate::store::relation_kinds;
use rusqlite::{Transaction, TransactionBehavior};

impl SqliteArtifactStore {
    pub(crate) fn append_current_compared(
        &self,
        artifacts: &[Artifact],
        schemas: &SchemaRegistry,
        expected: &[(SubjectId, SchemaId, Option<VersionId>)],
    ) -> Result<(), StoreError> {
        let mut seen = std::collections::HashSet::new();
        if !(1..=16).contains(&artifacts.len())
            || !(1..=16).contains(&expected.len())
            || expected
                .iter()
                .any(|(subject, schema, _)| !seen.insert((subject, schema)))
            || artifacts.iter().any(|artifact| {
                !expected.iter().any(|(subject, schema, _)| {
                    artifact.subject.as_ref() == Some(subject) && artifact.schema_id == *schema
                })
            })
        {
            return Err(StoreError::InvalidCurrentBatch);
        }
        for artifact in artifacts {
            validate_artifact(artifact, schemas).map_err(StoreError::Validation)?;
            if artifact.subject.is_none() {
                return Err(StoreError::MissingSubject);
            }
        }
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        for (subject, schema, expected) in expected {
            let actual: Option<String> = transaction
                .query_row(
                    "SELECT version_id FROM current_artifacts WHERE subject=?1 AND schema_id=?2",
                    params![subject.0, schema.0],
                    |row| row.get(0),
                )
                .optional()?;
            if actual.as_deref() != expected.as_ref().map(|id| id.0.as_str()) {
                return Err(StoreError::IdempotencyConflict);
            }
        }
        for artifact in artifacts {
            append_validated(&transaction, artifact)?;
            transaction.execute("INSERT INTO current_artifacts(subject,schema_id,version_id) VALUES(?1,?2,?3) ON CONFLICT(subject,schema_id) DO UPDATE SET version_id=excluded.version_id,updated_at=CURRENT_TIMESTAMP",
                params![artifact.subject.as_ref().unwrap().0,artifact.schema_id.0,artifact.version_id.0])?;
            if let Some(provenance) = &artifact.provenance {
                let relations = provenance
                    .parents
                    .iter()
                    .map(|id| (VersionId(id.clone()), relation_kinds::based_on()))
                    .collect::<Vec<_>>();
                insert_relations(&transaction, &artifact.version_id, &relations)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

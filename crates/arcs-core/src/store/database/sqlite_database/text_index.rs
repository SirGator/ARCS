//! Disposable projections in the same database as their immutable originals.
//! No schema-wide payload scan is performed when opening an existing store.
use super::*;
use crate::store::retrieval::{PayloadTextQuery, RetrievalError, RetrievalQuery};
use std::collections::BTreeMap;

pub(super) const MAX_ARTIFACT_BYTES: usize = 262_144;
const INDEX_BATCH: usize = 32;
const MAX_PROJECTIONS: usize = 128;

impl SqliteArtifactStore {
    /// Removes only disposable lexical projections. Original artifacts,
    /// relations and frozen memory receipts remain unchanged. The next fresh
    /// text recall rebuilds in bounded batches; cached recall needs no index.
    pub fn clear_text_indexes(&self) -> Result<(), StoreError> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        transaction.execute_batch(
            "DROP TABLE IF EXISTS artifact_text_fts_v1;
            DROP TABLE IF EXISTS artifact_text_documents_v1;
            DROP TABLE IF EXISTS artifact_text_projections_v1;",
        )?;
        transaction.commit()?;
        Ok(())
    }
}

/// Bounds SQLite VM work for text retrieval, including metadata filtering and
/// graph traversal. Removed before transaction rollback and before other ports.
pub(super) struct SearchWork<'a> {
    db: &'a Connection,
    exhausted: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl<'a> SearchWork<'a> {
    pub(super) fn new(db: &'a Connection) -> Result<Self, StoreError> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let exhausted = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&exhausted);
        let mut calls = 0;
        db.progress_handler(
            1000,
            Some(move || {
                calls += 1;
                if calls >= 1000 {
                    flag.store(true, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }),
        )?;
        Ok(Self { db, exhausted })
    }
    pub(super) fn exhausted(&self) -> bool {
        self.exhausted.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for SearchWork<'_> {
    fn drop(&mut self) {
        let _ = self.db.progress_handler(0, None::<fn() -> bool>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_work_is_interrupted_and_the_handler_is_removed_before_followup_work() {
        let store = SqliteArtifactStore::in_memory().unwrap();
        let work = SearchWork::new(&store.connection).unwrap();
        let result=store.connection.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<10000000) SELECT SUM(x) FROM n",[],|row|row.get::<_,i64>(0));
        assert!(result.is_err());
        assert!(work.exhausted());
        drop(work);
        let result:i64=store.connection.query_row("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<100000) SELECT SUM(x) FROM n",[],|row|row.get(0)).unwrap();
        assert_eq!(result, 5_000_050_000);
        store.durable_id_generator("after-interruption").unwrap();
    }
}

pub(super) fn key(schema: &SchemaId, text: &PayloadTextQuery) -> Result<String, StoreError> {
    let mut fields = text.fields.clone();
    fields.sort();
    Ok(serde_json::to_string(&(schema, fields))?)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextIndexTrace {
    pub schema_id: SchemaId,
    pub fields: Vec<String>,
    pub indexed_through: u64,
    pub target: u64,
    pub skipped_oversized: u64,
}

pub(super) fn prepare(
    db: &Connection,
    query: &RetrievalQuery,
    target: u64,
) -> Result<Vec<TextIndexTrace>, RetrievalError> {
    let mut projections = BTreeMap::new();
    for scope in &query.scopes {
        if let Some(text) = &scope.text {
            projections.insert(
                key(&scope.filter.schema_id, text)?,
                (&scope.filter.schema_id, text),
            );
        }
    }
    if projections.is_empty() {
        return Ok(vec![]);
    }
    db.execute_batch("CREATE TABLE IF NOT EXISTS artifact_text_projections_v1 (
        projection TEXT PRIMARY KEY, indexed_through INTEGER NOT NULL DEFAULT 0,
        skipped_oversized INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS artifact_text_documents_v1 (
        id INTEGER PRIMARY KEY, projection TEXT NOT NULL REFERENCES artifact_text_projections_v1(projection),
        version_sequence INTEGER NOT NULL REFERENCES artifact_versions(sequence),body TEXT NOT NULL,
        UNIQUE(projection,version_sequence));
        CREATE VIRTUAL TABLE IF NOT EXISTS artifact_text_fts_v1 USING fts5(
        body,content='artifact_text_documents_v1',content_rowid='id',tokenize='ascii');")
        .map_err(StoreError::from)?;
    let mut cursors = Vec::new();
    for (projection, (schema, text)) in &projections {
        let previous: Option<(u64,u64)> = db.query_row(
            "SELECT indexed_through,skipped_oversized FROM artifact_text_projections_v1 WHERE projection=?1",
            [projection], |row|Ok((row.get::<_, i64>(0)? as u64,row.get::<_, i64>(1)? as u64))).optional().map_err(StoreError::from)?;
        let (cursor, skipped) = if let Some(previous) = previous {
            previous
        } else {
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM artifact_text_projections_v1",
                    [],
                    |row| row.get(0),
                )
                .map_err(StoreError::from)?;
            if count >= MAX_PROJECTIONS as i64 {
                return Err(RetrievalError::TextIndexCapacity);
            }
            db.execute(
                "INSERT INTO artifact_text_projections_v1(projection) VALUES(?1)",
                [projection],
            )
            .map_err(StoreError::from)?;
            (0, 0)
        };
        cursors.push((cursor, projection, *schema, *text, skipped));
    }
    // Least advanced projections progress first; each call indexes <=32 artifacts
    // total, including old stores and policies with many different field sets.
    cursors.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let allowance = (INDEX_BATCH / cursors.len()).max(1);
    let mut traces = Vec::new();
    for (mut cursor, projection, schema, text, mut skipped) in cursors {
        if cursor < target {
            let mut statement=db.prepare("SELECT sequence,CASE WHEN length(CAST(artifact_json AS BLOB))<=?4 THEN artifact_json ELSE NULL END FROM artifact_versions WHERE schema_id=?1 AND sequence>?2 AND sequence<=?3 ORDER BY sequence LIMIT ?5").map_err(StoreError::from)?;
            let rows = statement
                .query_map(
                    params![
                        schema.0,
                        cursor as i64,
                        target as i64,
                        MAX_ARTIFACT_BYTES as i64,
                        (allowance + 1) as i64
                    ],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)? as u64,
                            row.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .map_err(StoreError::from)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreError::from)?;
            for (sequence, encoded) in rows.iter().take(allowance) {
                if let Some(encoded) = encoded {
                    let artifact: Artifact =
                        serde_json::from_str(encoded).map_err(StoreError::from)?;
                    db.execute("INSERT INTO artifact_text_documents_v1(projection,version_sequence,body) VALUES(?1,?2,?3)",params![projection,*sequence as i64,text.body(&artifact)]).map_err(StoreError::from)?;
                    let document = db.last_insert_rowid();
                    db.execute("INSERT INTO artifact_text_fts_v1(rowid,body) SELECT id,body FROM artifact_text_documents_v1 WHERE id=?1",[document]).map_err(StoreError::from)?;
                } else {
                    skipped += 1;
                }
                cursor = *sequence;
            }
            if rows.len() <= allowance {
                cursor = target;
            }
            db.execute("UPDATE artifact_text_projections_v1 SET indexed_through=?2,skipped_oversized=?3 WHERE projection=?1",params![projection,cursor as i64,skipped as i64]).map_err(StoreError::from)?;
        }
        traces.push(TextIndexTrace {
            schema_id: schema.clone(),
            fields: text.fields.clone(),
            indexed_through: cursor,
            target,
            skipped_oversized: skipped,
        });
    }
    Ok(traces)
}

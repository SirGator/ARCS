//! Snapshot-Abfragen des Graphen. Keine Reasoning- oder Geräteabhängigkeit.
use super::*;
use crate::store::ArtifactRelation;
use crate::store::retrieval::*;
use std::collections::{HashSet, VecDeque};

// Large payloads must not turn a bounded graph search into an unbounded read.
const MAX_RETRIEVED_ARTIFACT_BYTES: usize = super::text_index::MAX_ARTIFACT_BYTES;

impl SqliteArtifactStore {
    pub(crate) fn retrieve(
        &self,
        query: &RetrievalQuery,
    ) -> Result<RetrievalResult, RetrievalError> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.connection,
            if query.scopes.iter().any(|scope| scope.text.is_some()) {
                rusqlite::TransactionBehavior::Immediate
            } else {
                rusqlite::TransactionBehavior::Deferred
            },
        )
        .map_err(StoreError::from)?;
        let target: u64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM artifact_versions",
                [],
                |row| row.get::<_, i64>(0).map(|n| n as u64),
            )
            .map_err(StoreError::from)?;
        let indexes = super::text_index::prepare(&transaction, query, target)?;
        if let Some(pending) = indexes
            .iter()
            .filter(|index| index.indexed_through < target)
            .min_by_key(|index| index.indexed_through)
        {
            let indexed_through = pending.indexed_through;
            transaction.commit().map_err(StoreError::from)?;
            return Err(RetrievalError::TextIndexPending {
                indexed_through,
                target,
            });
        }
        let work = if indexes.is_empty() {
            None
        } else {
            Some(super::text_index::SearchWork::new(&transaction)?)
        };
        let retrieved = retrieve(&transaction, query);
        let exhausted = work.as_ref().is_some_and(|work| work.exhausted());
        drop(work);
        if exhausted {
            return Err(RetrievalError::TextSearchWorkLimit);
        }
        let mut result = retrieved?;
        result.trace.truncated |= indexes.iter().any(|index| index.skipped_oversized > 0);
        result.trace.text_indexes = indexes;
        transaction.commit().map_err(StoreError::from)?;
        Ok(result)
    }
}

fn retrieve(db: &Connection, query: &RetrievalQuery) -> Result<RetrievalResult, RetrievalError> {
    let cursor = |table: &str| -> Result<u64, StoreError> {
        db.query_row(
            &format!("SELECT COALESCE(MAX(sequence),0) FROM {table}"),
            [],
            |row| row.get::<_, i64>(0).map(|value| value as u64),
        )
        .map_err(StoreError::from)
    };
    let mut trace = RetrievalTrace {
        artifact_cursor: cursor("artifact_versions")?,
        relation_cursor: cursor("artifact_relations")?,
        visited: 0,
        edges_read: 0,
        truncated: false,
        text_indexes: vec![],
    };
    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    for seed in &query.seeds {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM artifact_versions WHERE version_id=?1 AND sequence<=?2)", params![seed.0, trace.artifact_cursor as i64], |row| row.get(0)).map_err(StoreError::from)?;
        if !exists {
            return Err(RetrievalError::MissingSeed(seed.clone()));
        }
        queued.insert(seed.clone());
        queue.push_back((seed.clone(), 0, vec![]));
    }
    // Only explicit subjects/tags seed a metadata search. A schema alone must
    // never implicitly expose unrelated projects from the shared history.
    for scope in query.scopes.iter().filter(|scope| scope.filter.is_scoped()) {
        let filter = &scope.filter;
        if let Some(text) = &scope.text {
            let projection = super::text_index::key(&filter.schema_id, text)?;
            let tags = serde_json::to_string(&filter.tags).map_err(StoreError::from)?;
            let remaining = query.limits.max_visited.saturating_sub(queue.len());
            let mut statement=db.prepare("SELECT v.version_id FROM artifact_text_fts_v1
                JOIN artifact_text_documents_v1 d ON d.id=artifact_text_fts_v1.rowid
                JOIN artifact_versions v ON v.sequence=d.version_sequence
                WHERE artifact_text_fts_v1 MATCH ?1 AND d.projection=?2
                AND v.schema_id=?3 AND (?4 IS NULL OR v.subject=?4) AND v.sequence<=?5
                AND NOT EXISTS(SELECT 1 FROM json_each(?6) wanted WHERE NOT EXISTS(
                    SELECT 1 FROM json_each(v.artifact_json,'$.tags') tag WHERE tag.value=wanted.value))
                ORDER BY v.sequence DESC LIMIT ?7").map_err(StoreError::from)?;
            let rows = statement
                .query_map(
                    params![
                        text.expression(),
                        projection,
                        filter.schema_id.0,
                        filter.subject.as_ref().map(|s| &s.0),
                        trace.artifact_cursor as i64,
                        tags,
                        (remaining + 1) as i64
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(StoreError::from)?;
            for row in rows {
                let version = VersionId(row.map_err(StoreError::from)?);
                if queued.contains(&version) {
                    continue;
                }
                if queue.len() >= query.limits.max_visited {
                    trace.truncated = true;
                    break;
                }
                queued.insert(version.clone());
                queue.push_back((version, 0, vec![]));
            }
            continue;
        }
        let mut statement = db
            .prepare(
                "SELECT version_id FROM artifact_versions v
            WHERE v.schema_id=?1 AND (?2 IS NULL OR v.subject=?2) AND v.sequence<=?3
            AND NOT EXISTS(SELECT 1 FROM json_each(?4) wanted WHERE NOT EXISTS(
                SELECT 1 FROM json_each(v.artifact_json,'$.tags') tag WHERE tag.value=wanted.value))
            ORDER BY v.sequence DESC LIMIT ?5",
            )
            .map_err(StoreError::from)?;
        let tags = serde_json::to_string(&filter.tags).map_err(StoreError::from)?;
        let remaining = query.limits.max_visited.saturating_sub(queue.len());
        let rows = statement
            .query_map(
                params![
                    filter.schema_id.0,
                    filter.subject.as_ref().map(|subject| &subject.0),
                    trace.artifact_cursor as i64,
                    tags,
                    (remaining + 1) as i64
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(StoreError::from)?;
        for row in rows {
            let version = VersionId(row.map_err(StoreError::from)?);
            if queued.contains(&version) {
                continue;
            }
            if queue.len() >= query.limits.max_visited {
                trace.truncated = true;
                break;
            }
            queued.insert(version.clone());
            queue.push_back((version, 0, vec![]));
        }
    }
    let kinds = serde_json::to_string(&query.relations).map_err(StoreError::from)?;
    let mut hits = Vec::new();
    while let Some((version, distance, path)) = queue.pop_front() {
        if trace.visited >= query.limits.max_visited {
            trace.truncated = true;
            break;
        }
        trace.visited += 1;
        let node: (u64, Option<String>) = db
            .query_row(
                "SELECT sequence,
            CASE WHEN length(CAST(artifact_json AS BLOB))<=?2 THEN artifact_json ELSE NULL END
            FROM artifact_versions WHERE version_id=?1",
                params![version.0, MAX_RETRIEVED_ARTIFACT_BYTES as i64],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
            )
            .map_err(StoreError::from)?;
        if let Some(encoded) = node.1 {
            let artifact: Artifact = serde_json::from_str(&encoded).map_err(StoreError::from)?;
            if query.scopes.iter().any(|scope| scope.matches(&artifact)) {
                hits.push(RetrievalHit {
                    artifact,
                    sequence: node.0,
                    distance,
                    path: path.clone(),
                });
                if hits.len() > query.limits.max_candidates {
                    hits.sort_by(|a, b| {
                        a.distance
                            .cmp(&b.distance)
                            .then_with(|| b.sequence.cmp(&a.sequence))
                            .then_with(|| a.artifact.version_id.0.cmp(&b.artifact.version_id.0))
                    });
                    hits.truncate(query.limits.max_candidates);
                    trace.truncated = true;
                }
            }
        } else {
            trace.truncated = true;
        }
        if distance >= query.limits.max_depth || query.relations.is_empty() {
            continue;
        }
        let remaining = query.limits.max_edges.saturating_sub(trace.edges_read);
        if remaining == 0 {
            trace.truncated = true;
            continue;
        }
        let mut statement = db
            .prepare(
                "SELECT from_version_id,to_version_id,relation_kind
            FROM artifact_relations WHERE (from_version_id=?1 OR to_version_id=?1)
            AND sequence<=?2 AND relation_kind IN (SELECT value FROM json_each(?3))
            ORDER BY sequence LIMIT ?4",
            )
            .map_err(StoreError::from)?;
        let rows = statement
            .query_map(
                params![
                    version.0,
                    trace.relation_cursor as i64,
                    kinds,
                    (remaining + 1) as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(StoreError::from)?;
        for row in rows {
            if trace.edges_read >= query.limits.max_edges {
                trace.truncated = true;
                break;
            }
            let (from, to, kind) = row.map_err(StoreError::from)?;
            trace.edges_read += 1;
            let relation = ArtifactRelation {
                from: VersionId(from),
                to: VersionId(to),
                kind: RelationKind::new(kind).map_err(|_| RetrievalError::InvalidQuery)?,
            };
            let neighbor = if relation.from == version {
                relation.to.clone()
            } else {
                relation.from.clone()
            };
            if queued.contains(&neighbor) {
                continue;
            }
            if queued.len() >= query.limits.max_visited {
                trace.truncated = true;
                continue;
            }
            let mut next_path = path.clone();
            next_path.push(relation);
            queued.insert(neighbor.clone());
            queue.push_back((neighbor, distance + 1, next_path));
        }
    }
    hits.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then_with(|| b.sequence.cmp(&a.sequence))
            .then_with(|| a.artifact.version_id.0.cmp(&b.artifact.version_id.0))
    });
    if hits.len() > query.limits.max_candidates {
        trace.truncated = true;
        hits.truncate(query.limits.max_candidates);
    }
    Ok(RetrievalResult { hits, trace })
}

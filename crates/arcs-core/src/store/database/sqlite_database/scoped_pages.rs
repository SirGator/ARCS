//! Indexed, bounded history pages; skipped large rows still have a cursor.
use super::*;

#[derive(Debug)]
pub struct ScopedArtifactRow {
    pub sequence: u64,
    pub artifact: Option<Artifact>,
}

impl SqliteArtifactStore {
    /// Latest immutable scoped row at a fixed log cursor. A too large row is
    /// returned without its body; it must never masquerade as missing evidence.
    pub fn schema_subject_head_at(
        &self,
        schema: &SchemaId,
        subject: &SubjectId,
        through: u64,
        max_encoded_bytes: usize,
    ) -> Result<Option<ScopedArtifactRow>, StoreError> {
        if through > i64::MAX as u64 || !(1..=262_144).contains(&max_encoded_bytes) {
            return Err(StoreError::InvalidPageLimits);
        }
        let row: Option<(u64, Option<String>)> = self.connection.query_row(
            "SELECT sequence,CASE WHEN length(CAST(artifact_json AS BLOB))<=?4 THEN artifact_json ELSE NULL END
             FROM artifact_versions INDEXED BY idx_artifact_versions_schema_subject_sequence
             WHERE schema_id=?1 AND subject=?2 AND sequence<=?3 ORDER BY sequence DESC LIMIT 1",
            params![schema.0, subject.0, through as i64, max_encoded_bytes as i64],
            |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
        ).optional()?;
        row.map(|(sequence, body)| {
            Ok(ScopedArtifactRow {
                sequence,
                artifact: body.map(|body| serde_json::from_str(&body)).transpose()?,
            })
        })
        .transpose()
    }
    pub fn schema_subject_page_after(
        &self,
        schema: &SchemaId,
        subject: &SubjectId,
        after: u64,
        through: u64,
        limit: usize,
        max_encoded_bytes: usize,
    ) -> Result<Vec<ScopedArtifactRow>, StoreError> {
        if after > i64::MAX as u64
            || through > i64::MAX as u64
            || !(1..=257).contains(&limit)
            || !(1..=262_144).contains(&max_encoded_bytes)
        {
            return Err(StoreError::InvalidPageLimits);
        }
        let mut statement=self.connection.prepare("SELECT sequence,CASE WHEN length(CAST(artifact_json AS BLOB))<=?4 THEN artifact_json ELSE NULL END FROM artifact_versions INDEXED BY idx_artifact_versions_schema_subject_sequence WHERE schema_id=?1 AND subject=?2 AND sequence>?3 AND sequence<=?6 ORDER BY sequence LIMIT ?5")?;
        let rows = statement.query_map(
            params![
                schema.0,
                subject.0,
                after as i64,
                max_encoded_bytes as i64,
                limit as i64,
                through as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (sequence, encoded) = row?;
            Ok(ScopedArtifactRow {
                sequence,
                artifact: encoded
                    .map(|text| serde_json::from_str(&text))
                    .transpose()?,
            })
        })
        .collect()
    }
}

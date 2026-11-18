//! Read selected immutable metadata without materializing whole request bodies.
use super::*;
use crate::core::{Actor, Source, Trust};

#[derive(Debug, serde::Deserialize)]
pub struct ArtifactMetadata {
    pub artifact_id: crate::core::ArtifactId,
    pub version_id: VersionId,
    pub version: u64,
    pub schema_id: SchemaId,
    pub created_by: Actor,
    pub source: Source,
    pub trust: Trust,
    pub fields: serde_json::Value,
}
#[derive(Clone, Copy)]
pub enum MetadataValue {
    Value,
    ArrayLength,
    StringPrefix(usize),
}
pub struct MetadataField<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub value: MetadataValue,
}
impl<'a> MetadataField<'a> {
    pub fn value(name: &'a str, path: &'a str) -> Self {
        Self {
            name,
            path,
            value: MetadataValue::Value,
        }
    }
    pub fn array_length(name: &'a str, path: &'a str) -> Self {
        Self {
            name,
            path,
            value: MetadataValue::ArrayLength,
        }
    }
    pub fn prefix(name: &'a str, path: &'a str, characters: usize) -> Self {
        Self {
            name,
            path,
            value: MetadataValue::StringPrefix(characters),
        }
    }
}

impl SqliteArtifactStore {
    pub fn metadata_projection(
        &self,
        version: &VersionId,
        fields: &[MetadataField<'_>],
        max_bytes: usize,
    ) -> Result<Option<ArtifactMetadata>, StoreError> {
        let mut names = std::collections::HashSet::new();
        if !(1..=16).contains(&fields.len())
            || !(1..=32_768).contains(&max_bytes)
            || fields.iter().any(|field| {
                field.name.is_empty()
                    || field.name.len() > 128
                    || !names.insert(field.name)
                    || !field.path.starts_with("$.")
                    || field.path.len() > 512
                    || field.path.chars().any(char::is_control)
                    || matches!(field.value,MetadataValue::StringPrefix(n) if n==0 || n>512)
            })
        {
            return Err(StoreError::InvalidPageLimits);
        }
        let mut values = vec![rusqlite::types::Value::Text(version.0.clone())];
        let mut expressions = Vec::new();
        for field in fields {
            values.push(field.name.to_string().into());
            let name = values.len();
            values.push(field.path.to_string().into());
            let path = values.len();
            let value = match field.value {
                MetadataValue::Value => format!("json_extract(artifact_json,?{path})"),
                MetadataValue::ArrayLength => format!(
                    "CASE WHEN json_type(artifact_json,?{path})='array' THEN json_array_length(artifact_json,?{path}) ELSE NULL END"
                ),
                MetadataValue::StringPrefix(n) => {
                    format!("substr(json_extract(artifact_json,?{path}),1,{n})")
                }
            };
            expressions.push(format!("?{name},{value}"));
        }
        values.push((max_bytes as i64).into());
        let limit = values.len();
        let sql=format!("SELECT CASE WHEN length(CAST(projected AS BLOB))<=?{limit} THEN projected ELSE NULL END FROM (
            SELECT json_object('artifact_id',artifact_id,'version_id',version_id,'version',version,'schema_id',schema_id,
            'created_by',json_extract(artifact_json,'$.created_by'),'source',json_extract(artifact_json,'$.source'),
            'trust',json_extract(artifact_json,'$.trust'),'fields',json_object({})) projected
            FROM artifact_versions WHERE version_id=?1)",expressions.join(","));
        let encoded: Option<Option<String>> = self
            .connection
            .query_row(&sql, rusqlite::params_from_iter(values), |row| row.get(0))
            .optional()?;
        match encoded {
            None => Ok(None),
            Some(None) => Err(StoreError::ProjectionTooLarge),
            Some(Some(encoded)) => Ok(Some(serde_json::from_str(&encoded)?)),
        }
    }
}

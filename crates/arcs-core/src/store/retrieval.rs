//! Begrenzter Abruf im bestehenden Artefaktgraphen, ohne eigenen Datenbestand.
use serde::{Deserialize, Serialize};

use super::{ArtifactRelation, RelationKind, SqliteArtifactStore, StoreError};
use crate::core::{Artifact, SchemaId, SubjectId, VersionId};

pub mod text;
pub use super::database::sqlite_database::TextIndexTrace;
pub use text::PayloadTextQuery;

#[derive(Debug, Clone)]
pub struct RetrievalScope {
    pub filter: ArtifactFilter,
    pub text: Option<PayloadTextQuery>,
}
impl RetrievalScope {
    pub fn matches(&self, artifact: &Artifact) -> bool {
        self.filter.matches(artifact)
            && self.text.as_ref().is_none_or(|text| text.matches(artifact))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactFilter {
    pub schema_id: SchemaId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}
impl ArtifactFilter {
    pub fn matches(&self, artifact: &Artifact) -> bool {
        artifact.schema_id == self.schema_id
            && self
                .subject
                .as_ref()
                .is_none_or(|subject| artifact.subject.as_ref() == Some(subject))
            && self.tags.iter().all(|tag| artifact.tags.contains(tag))
    }
    pub fn is_scoped(&self) -> bool {
        self.subject.is_some() || !self.tags.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalLimits {
    pub max_depth: usize,
    pub max_visited: usize,
    pub max_edges: usize,
    pub max_candidates: usize,
}
impl RetrievalLimits {
    pub fn valid(&self) -> bool {
        self.max_depth <= 8
            && (1..=2048).contains(&self.max_visited)
            && (1..=8192).contains(&self.max_edges)
            && (1..=256).contains(&self.max_candidates)
    }
}

pub struct RetrievalQuery {
    pub seeds: Vec<VersionId>,
    pub scopes: Vec<RetrievalScope>,
    pub relations: Vec<RelationKind>,
    pub limits: RetrievalLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalTrace {
    pub artifact_cursor: u64,
    pub relation_cursor: u64,
    pub visited: usize,
    pub edges_read: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_indexes: Vec<TextIndexTrace>,
}
#[derive(Debug, Clone)]
pub struct RetrievalHit {
    pub artifact: Artifact,
    pub sequence: u64,
    pub distance: usize,
    pub path: Vec<ArtifactRelation>,
}
pub struct RetrievalResult {
    pub hits: Vec<RetrievalHit>,
    pub trace: RetrievalTrace,
}

#[derive(Debug)]
pub enum RetrievalError {
    Store(StoreError),
    InvalidQuery,
    MissingSeed(VersionId),
    /// A bounded batch was committed. Retry without freezing a partial context.
    TextIndexPending {
        indexed_through: u64,
        target: u64,
    },
    TextIndexCapacity,
    TextSearchWorkLimit,
}
impl From<StoreError> for RetrievalError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

pub struct ArtifactRetrieval<'a> {
    store: &'a SqliteArtifactStore,
}
impl<'a> ArtifactRetrieval<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
    }

    /// Scoped metadata supplies entry points; unscoped filters only admit
    /// artifacts reachable from explicit seeds. Relations are read both ways.
    pub fn recall(&self, query: &RetrievalQuery) -> Result<RetrievalResult, RetrievalError> {
        let mut seeds = std::collections::HashSet::new();
        let mut relations = std::collections::HashSet::new();
        if !query.limits.valid()
            || query.seeds.len() > 128
            || query.scopes.is_empty()
            || query.scopes.len() > 16
            || query
                .scopes
                .iter()
                .any(|scope| scope.text.as_ref().is_some_and(|text| !text.valid()))
            || query.relations.len() > 32
            || query.seeds.iter().any(|seed| !seeds.insert(seed))
            || query
                .relations
                .iter()
                .any(|kind| RelationKind::new(kind.as_str()).is_err() || !relations.insert(kind))
            || query
                .scopes
                .iter()
                .map(|scope| &scope.filter)
                .any(|filter| {
                    filter.schema_id.0.is_empty()
                        || filter.schema_id.0.len() > 256
                        || filter.tags.len() > 32
                        || filter.tags.iter().any(|tag| {
                            tag.is_empty() || tag.len() > 1024 || tag.chars().any(char::is_control)
                        })
                        || filter.subject.as_ref().is_some_and(|subject| {
                            subject.0.is_empty()
                                || subject.0.len() > 512
                                || subject.0.chars().any(char::is_control)
                        })
                })
        {
            return Err(RetrievalError::InvalidQuery);
        }
        self.store.retrieve(query)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;

//! Exact lexical AND over explicitly selected, top-level string fields.
//! Normalization is shared by the derived SQLite index and replay validation.
use crate::core::Artifact;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadTextQuery {
    pub query: String,
    pub fields: Vec<String>,
}

pub fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .flat_map(|word| {
            word.to_lowercase()
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
}

impl PayloadTextQuery {
    pub fn valid(&self) -> bool {
        if self.query.is_empty()
            || self.query.len() > 4096
            || self.query.chars().any(char::is_control)
        {
            return false;
        }
        let mut fields = HashSet::new();
        let terms: Vec<_> = words(&self.query).collect();
        (1..=32).contains(&terms.len())
            && terms.iter().all(|term| term.len() <= 128)
            && (1..=64).contains(&self.fields.len())
            && self.fields.iter().all(|field| {
                !field.trim().is_empty()
                    && field.len() <= 256
                    && !field.chars().any(char::is_control)
                    && fields.insert(field)
            })
    }

    pub fn matches(&self, artifact: &Artifact) -> bool {
        if !self.valid() {
            return false;
        }
        let body = self.body(artifact);
        let present: HashSet<_> = words(&body).collect();
        words(&self.query).all(|term| present.contains(&term))
    }

    pub(crate) fn body(&self, artifact: &Artifact) -> String {
        self.fields
            .iter()
            .filter_map(|field| {
                artifact
                    .payload
                    .get(field)
                    .and_then(serde_json::Value::as_str)
            })
            .flat_map(words)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(crate) fn expression(&self) -> String {
        words(&self.query)
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

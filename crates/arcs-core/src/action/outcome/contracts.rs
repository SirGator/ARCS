use serde::{Deserialize, Serialize};

use crate::core::Artifact;

use super::OutcomeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeVerdict {
    Success,
    Failure,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeResult {
    pub verdict: OutcomeVerdict,
    pub detail: String,
}

/// Fachlicher Bewertungsport ohne Zugriff auf Store oder Lernlogik.
pub trait OutcomeEvaluator {
    fn evaluate(&self, execution_result: &Artifact) -> Result<OutcomeResult, OutcomeError>;
}

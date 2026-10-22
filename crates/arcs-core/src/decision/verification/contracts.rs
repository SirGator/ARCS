use serde::{Deserialize, Serialize};

use crate::core::Artifact;

use super::VerificationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationVerdict {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationFinding {
    pub check: String,
    pub verdict: VerificationVerdict,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationResult {
    pub verdict: VerificationVerdict,
    pub findings: Vec<VerificationFinding>,
}

/// Fachlicher Prüfport ohne Zugriff auf Store oder Runtime-Autorität.
pub trait Verifier {
    fn verify(&self, artifact: &Artifact) -> Result<VerificationResult, VerificationError>;
}

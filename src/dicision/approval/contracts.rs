use serde::{Deserialize, Serialize};

/// Abgeschlossene Entscheidung am Autoritäts-Gate vor einer späteren Execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Rejected,
}

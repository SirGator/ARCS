use serde::{Deserialize, Serialize};

/// Dauerhafter Lebenszyklus eines externen oder wirkungsbehafteten Aufrufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationStatus {
    Prepared,
    Dispatched,
    Succeeded,
    Failed,
    Unknown,
}

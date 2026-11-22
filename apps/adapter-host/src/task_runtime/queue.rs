//! Read-only, bounded pages over the active runtime journal.
use arcs::core::VersionId;
use serde::{Deserialize, Serialize};
use std::collections::btree_map::{BTreeMap, Entry};

use super::{TaskRuntime, config::invalid};
use crate::HostError;

pub(crate) const MAX_TASK_QUEUE_PAGE: usize = 100;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskQueueScope {
    #[default]
    Open,
    All,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskQueueQuery {
    #[serde(default)]
    pub scope: TaskQueueScope,
    #[serde(default)]
    pub after_sequence: u64,
    #[serde(default)]
    pub through_sequence: Option<u64>,
    pub limit: usize,
}

impl TaskQueueQuery {
    fn validate(&self) -> Result<(), HostError> {
        if !(1..=MAX_TASK_QUEUE_PAGE).contains(&self.limit)
            || self.after_sequence > i64::MAX as u64
            || self
                .through_sequence
                .is_some_and(|end| end < self.after_sequence || end > i64::MAX as u64)
        {
            return Err(invalid("invalid task queue page size or sequence range"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskWait {
    Time,
    Event,
    Dependency,
    Approval,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ReasoningRequestBudget {
    pub task_sequence: u64,
    pub used: u64,
    pub limit: u32,
}

#[derive(Debug, Serialize)]
pub struct TaskQueueRow {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub priority: u8,
    pub state: String,
    pub goal_state: Option<String>,
    pub waiting_for: Option<TaskWait>,
    pub not_before_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_at_unix_ms: Option<u64>,
    pub deadline_unix_ms: Option<u64>,
    pub parent_plan_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_budget: Option<ReasoningRequestBudget>,
    pub series_sequence: Option<u64>,
    pub control_revision: u64,
    pub locally_paused: bool,
    pub paused: bool,
    pub can_control: bool,
    pub action_recovery_required: bool,
}

#[derive(Debug, Serialize)]
pub struct TaskQueuePage {
    pub scope: TaskQueueScope,
    pub after_sequence: u64,
    pub through_sequence: u64,
    pub next_after_sequence: Option<u64>,
    pub tasks: Vec<TaskQueueRow>,
}

impl TaskRuntime {
    /// The initial page captures the largest existing sequence. Later pages
    /// reuse it while reporting the current state of each matching task.
    pub fn task_queue(&self, query: TaskQueueQuery) -> Result<TaskQueuePage, HostError> {
        query.validate()?;
        let mut page = self.journal.task_queue(query)?;
        if self.has_reasoning_request_limits {
            let mut scopes = BTreeMap::new();
            for task in &mut page.tasks {
                let scope = task.parent_plan_sequence.unwrap_or(task.sequence);
                task.reasoning_budget = match scopes.entry(scope) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        let budget = if let Some(limit) = self.reasoning_request_limit(scope)? {
                            Some(ReasoningRequestBudget {
                                task_sequence: scope,
                                used: self.journal.reasoning_request_count(scope)?,
                                limit,
                            })
                        } else {
                            None
                        };
                        *entry.insert(budget)
                    }
                };
            }
        }
        Ok(page)
    }
}

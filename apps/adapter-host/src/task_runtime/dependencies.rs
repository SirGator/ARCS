//! Explizite DAG-Voraussetzungen derselben Eingabe; keine zusätzliche Autorität.
mod config;

use arcs::core::{Artifact, SchemaId, VersionId};
use arcs::reasoning::{ContextSelection, ReasoningStatus};
use serde::{Deserialize, Serialize};

use super::{
    CompletedTask, TaskRuntime,
    config::invalid,
    goal::{GoalState, GoalTask},
};
use crate::{Host, HostError};

pub(super) use config::validate as validate_routes;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDependency {
    pub route_id: String,
    pub payload_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyBinding {
    pub sequence: u64,
    pub route_id: String,
    pub result_version_id: VersionId,
    pub context: Vec<ContextSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyState {
    WaitingDependency,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyReceipt {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub state: DependencyState,
    pub bindings: Vec<DependencyBinding>,
    pub detail: Option<String>,
}

pub(super) struct ParentTask {
    pub sequence: u64,
    pub route_id: String,
    pub input: VersionId,
    pub state: String,
    pub result: Option<String>,
    pub goal: Option<String>,
    pub fields: Vec<String>,
}
pub(super) enum ParentResult {
    Waiting,
    Blocked(String),
    Ready(DependencyBinding),
}

impl TaskRuntime {
    pub(super) fn advance_dependencies(&mut self, host: &Host) -> Result<(), HostError> {
        let mut visited = std::collections::HashSet::new();
        for _ in 0..self.config.max_jobs_per_tick {
            let Some(sequence) = self.journal.next_dependency_wait()? else {
                break;
            };
            if !visited.insert(sequence) {
                break;
            }
            let parents = self.journal.dependency_parents(sequence)?;
            if parents.is_empty() {
                return Err(invalid("dependency edges are missing"));
            }
            let mut bindings = Vec::new();
            let mut waiting = false;
            let mut blocked = None;
            for parent in parents {
                match self.resolve_parent(host, parent)? {
                    ParentResult::Waiting => waiting = true,
                    ParentResult::Blocked(reason) => {
                        blocked = Some(reason);
                        break;
                    }
                    ParentResult::Ready(binding) => bindings.push(binding),
                }
            }
            self.journal.finish_dependency_scan(
                sequence,
                if waiting { None } else { Some(&bindings) },
                blocked.as_deref(),
            )?;
        }
        Ok(())
    }

    pub(super) fn resolve_parent(
        &self,
        host: &Host,
        parent: ParentTask,
    ) -> Result<ParentResult, HostError> {
        let route = self.route_for_job(host, parent.sequence, &parent.route_id)?;
        let block = |message: &str| {
            ParentResult::Blocked(format!("dependency {}: {message}", parent.route_id))
        };
        let (result_version_id, versions, producer, schema) = match parent.state.as_str() {
            "pending" | "running" | "awaiting_approval" => return Ok(ParentResult::Waiting),
            "failed"
                if self.config.retry_failed_jobs()
                    && self.journal.job_is_runnable(parent.sequence, true)? =>
            {
                return Ok(ParentResult::Waiting);
            }
            "failed" | "blocked" | "rejected" | "budget_exhausted" | "cancelled" => {
                return Ok(block("prerequisite did not complete successfully"));
            }
            "ready_for_review" if route.goal.is_none() => {
                let saved: CompletedTask = serde_json::from_str(
                    parent
                        .result
                        .as_deref()
                        .ok_or_else(|| invalid("missing dependency completion"))?,
                )
                .map_err(|e| invalid(format!("invalid dependency completion: {e}")))?;
                if saved.sequence != parent.sequence
                    || saved.route_id != parent.route_id
                    || saved.input_version_id != parent.input
                {
                    return Err(invalid("dependency completion identity changed"));
                }
                if saved.reasoning_status != ReasoningStatus::Solved
                    || saved.candidate_version_ids.is_empty()
                {
                    return Ok(block("reasoning produced no solved candidates"));
                }
                let producer = self
                    .completed_reasoner(host, &route, &saved)?
                    .capability
                    .adapter_id
                    .0;
                (
                    saved.result_version_id,
                    saved.candidate_version_ids,
                    producer,
                    route.target_schema_id.clone(),
                )
            }
            "completed" if route.goal.is_some() => {
                let saved: GoalTask = serde_json::from_str(
                    parent
                        .goal
                        .as_deref()
                        .ok_or_else(|| invalid("missing dependency goal checkpoint"))?,
                )
                .map_err(|e| invalid(format!("invalid dependency goal: {e}")))?;
                if saved.sequence != parent.sequence
                    || saved.route_id != parent.route_id
                    || saved.input_version_id != parent.input
                    || saved.state != GoalState::Completed
                {
                    return Err(invalid("dependency goal identity changed"));
                }
                let assessment = saved
                    .assessment
                    .ok_or_else(|| invalid("completed dependency goal has no assessment"))?;
                (
                    assessment.clone(),
                    vec![assessment],
                    route
                        .goal
                        .as_ref()
                        .unwrap()
                        .assessment
                        .capability
                        .adapter_id
                        .0
                        .clone(),
                    SchemaId(super::goal::ASSESSMENT_SCHEMA.into()),
                )
            }
            _ => return Err(invalid("invalid dependency parent state")),
        };
        let mut context = Vec::new();
        for version in versions {
            let artifact = load(host, &version)?;
            if artifact.schema_id != schema || artifact.created_by.id != producer {
                return Err(invalid("dependency result schema or producer changed"));
            }
            if parent
                .fields
                .iter()
                .any(|field| artifact.payload.get(field).is_none())
            {
                return Ok(block("a selected result field is missing"));
            }
            context.push(ContextSelection {
                version_id: version,
                payload_fields: parent.fields.clone(),
            });
        }
        Ok(ParentResult::Ready(DependencyBinding {
            sequence: parent.sequence,
            route_id: parent.route_id,
            result_version_id,
            context,
        }))
    }

    pub fn dependencies_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<DependencyReceipt>, HostError> {
        self.journal.dependencies_after(sequence, limit)
    }
}
fn load(host: &Host, version: &VersionId) -> Result<Artifact, HostError> {
    host.store()
        .get(version)
        .map_err(HostError::Store)?
        .ok_or_else(|| invalid("dependency artifact is missing"))
}

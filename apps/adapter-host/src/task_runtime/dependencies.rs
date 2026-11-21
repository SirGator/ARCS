//! Explizite DAG-Voraussetzungen derselben Eingabe; keine zusätzliche Autorität.
mod config;

use arcs::core::{ActorType, Artifact, SchemaId, VersionId};
use arcs::execution::execution_invocation_id;
use arcs::outcome::OutcomeVerdict;
use arcs::reasoning::{ContextSelection, ReasoningStatus};
use arcs::store::{ArtifactRelations, relation_kinds};
use serde::{Deserialize, Serialize};

use super::{
    CompletedTask, TaskRuntime,
    config::invalid,
    goal::{ASSESSMENT_SCHEMA, GoalAssessment, GoalTask, GoalVerdict},
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failure_evidence: bool,
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
            let Some((sequence, mut bindings)) = self.journal.next_dependency_wait()? else {
                break;
            };
            if !visited.insert(sequence) {
                break;
            }
            let parents = self.journal.dependency_parents(sequence)?;
            if parents.is_empty() {
                return Err(invalid("dependency edges are missing"));
            }
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
        match parent.state.as_str() {
            "pending" | "running" | "awaiting_approval" => Ok(ParentResult::Waiting),
            "failed" | "blocked" | "rejected" | "budget_exhausted" | "cancelled" => Ok(
                blocked_parent(&parent, "prerequisite did not complete successfully"),
            ),
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
                    return Ok(blocked_parent(
                        &parent,
                        "reasoning produced no solved candidates",
                    ));
                }
                let producer = self
                    .completed_reasoner(host, &route, &saved)?
                    .capability
                    .adapter_id
                    .0;
                let artifacts = saved
                    .candidate_version_ids
                    .iter()
                    .map(|version| load_result(host, version, &producer, &route.target_schema_id));
                bind_parent(&parent, saved.result_version_id, artifacts)
            }
            "completed" if route.goal.is_some() => {
                let saved = parent_goal(&parent)?
                    .ok_or_else(|| invalid("missing dependency goal checkpoint"))?;
                let goal = route.goal.as_ref().expect("goal route");
                let assessment = saved
                    .assessment
                    .as_ref()
                    .ok_or_else(|| invalid("completed dependency goal has no assessment"))?;
                let assessor = &goal.assessment.capability.adapter_id.0;
                let artifact = if let Some(schema) = &goal.export_execution_schema_id {
                    assessed_goal_receipt(
                        host,
                        &saved,
                        assessment,
                        assessor,
                        schema,
                        OutcomeVerdict::Success,
                    )?
                    .ok_or_else(|| invalid("completed goal export lacks a successful assessment"))
                } else {
                    load_result(
                        host,
                        assessment,
                        assessor,
                        &SchemaId(ASSESSMENT_SCHEMA.into()),
                    )
                };
                bind_parent(&parent, assessment.clone(), std::iter::once(artifact))
            }
            _ => Err(invalid("invalid dependency parent state")),
        }
    }

    pub fn dependencies_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<DependencyReceipt>, HostError> {
        self.journal.dependencies_after(sequence, limit)
    }
}
fn bind_parent(
    parent: &ParentTask,
    result_version_id: VersionId,
    artifacts: impl Iterator<Item = Result<Artifact, HostError>>,
) -> Result<ParentResult, HostError> {
    let mut context = Vec::new();
    for artifact in artifacts {
        let artifact = artifact?;
        if parent
            .fields
            .iter()
            .any(|field| artifact.payload.get(field).is_none())
        {
            return Ok(blocked_parent(parent, "a selected result field is missing"));
        }
        context.push(ContextSelection {
            version_id: artifact.version_id,
            payload_fields: parent.fields.clone(),
        });
    }
    Ok(ParentResult::Ready(DependencyBinding {
        sequence: parent.sequence,
        route_id: parent.route_id.clone(),
        result_version_id,
        failure_evidence: false,
        context,
    }))
}

fn blocked_parent(parent: &ParentTask, message: &str) -> ParentResult {
    ParentResult::Blocked(format!("dependency {}: {message}", parent.route_id))
}

fn parent_goal(parent: &ParentTask) -> Result<Option<GoalTask>, HostError> {
    let Some(raw) = &parent.goal else {
        return Ok(None);
    };
    let task: GoalTask = serde_json::from_str(raw)
        .map_err(|error| invalid(format!("invalid dependency goal: {error}")))?;
    if task.sequence != parent.sequence
        || task.route_id != parent.route_id
        || task.input_version_id != parent.input
        || task.state.queue_state() != parent.state
    {
        return Err(invalid("dependency goal identity changed"));
    }
    Ok(Some(task))
}

/// Collect evidence for replanning. The failed parent stays blocked
/// for ordinary dependencies and completed-result reuse.
pub(super) fn failed_goal_receipt(
    host: &Host,
    parent: &ParentTask,
    goal: &super::goal::GoalConfig,
    fields: &[String],
) -> Result<Option<ContextSelection>, HostError> {
    if fields.is_empty() || !matches!(parent.state.as_str(), "blocked" | "budget_exhausted") {
        return Ok(None);
    }
    let Some(schema) = &goal.export_execution_schema_id else {
        return Err(invalid("failure export needs an execution export schema"));
    };
    let Some(task) = parent_goal(parent)? else {
        return Ok(None);
    };
    // Only share the last assessed execution retained by this task.
    // A later round can have failed before dispatching an action.
    let (Some(assessment), Some(_), Some(outcome)) =
        (&task.assessment, &task.execution, &task.outcome)
    else {
        return Ok(None);
    };
    let receipt = assessed_goal_receipt(
        host,
        &task,
        assessment,
        &goal.assessment.capability.adapter_id.0,
        schema,
        OutcomeVerdict::Failure,
    )?;
    if let Some(receipt) = receipt
        && fields
            .iter()
            .all(|field| receipt.payload.get(field).is_some())
    {
        let outcome = load(host, outcome)?;
        if outcome.schema_id.0 != "arcs.outcome.v1"
            || outcome.created_by.actor_type != ActorType::System
            || outcome.created_by.id != "arcs.outcome"
            || outcome.stream_key != format!("outcome:{}", receipt.version_id.0)
            || outcome.payload["execution_result_version"] != receipt.version_id.0
            || outcome.payload["verdict"] != "failure"
        {
            return Err(invalid("failed receipt does not match the saved outcome"));
        }
        return Ok(Some(ContextSelection {
            version_id: receipt.version_id,
            payload_fields: fields.to_vec(),
        }));
    }
    Ok(None)
}

fn assessed_goal_receipt(
    host: &Host,
    task: &GoalTask,
    assessment: &VersionId,
    assessor: &str,
    schema: &SchemaId,
    expected: OutcomeVerdict,
) -> Result<Option<Artifact>, HostError> {
    let execution = task
        .execution
        .as_ref()
        .ok_or_else(|| invalid("completed export goal has no execution receipt"))?;
    let action = task
        .action
        .as_ref()
        .ok_or_else(|| invalid("execution export has no action"))?;
    let capability = task
        .capability
        .as_ref()
        .ok_or_else(|| invalid("execution export has no capability"))?;
    let assessment = load_result(
        host,
        assessment,
        assessor,
        &SchemaId(ASSESSMENT_SCHEMA.into()),
    )?;
    let verdict: GoalAssessment = serde_json::from_value(assessment.payload)
        .map_err(|error| invalid(format!("invalid execution export assessment: {error}")))?;
    if verdict.effect != expected
        || (expected == OutcomeVerdict::Success && verdict.goal_status != GoalVerdict::Achieved)
    {
        return Ok(None);
    }
    if !verdict.evidence_version_ids.contains(execution) {
        return Err(invalid(
            "execution export was not confirmed by the completed assessment",
        ));
    }
    let artifact = load(host, execution)?;
    let expected_stream = format!(
        "execution:{}",
        execution_invocation_id(action, capability, schema)
    );
    if &artifact.schema_id != schema
        || artifact.created_by.actor_type != ActorType::Executor
        || artifact.created_by.id != capability.adapter_id.0
        || artifact.stream_key != expected_stream
        || !ArtifactRelations::new(host.store())
            .outgoing(execution)
            .map_err(HostError::Store)?
            .iter()
            .any(|edge| &edge.to == action && edge.kind == relation_kinds::result_of())
    {
        return Err(invalid(
            "execution export does not match the approved action",
        ));
    }
    Ok(Some(artifact))
}

fn load_result(
    host: &Host,
    version: &VersionId,
    producer: &str,
    schema: &SchemaId,
) -> Result<Artifact, HostError> {
    let artifact = load(host, version)?;
    if &artifact.schema_id != schema || artifact.created_by.id != producer {
        return Err(invalid("dependency result schema or producer changed"));
    }
    Ok(artifact)
}

fn load(host: &Host, version: &VersionId) -> Result<Artifact, HostError> {
    host.store()
        .get(version)
        .map_err(HostError::Store)?
        .ok_or_else(|| invalid("dependency artifact is missing"))
}

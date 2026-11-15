//! Begrenzte Reasoning-Pläne aus Betreiber-Vorlagen; keine neuen Berechtigungen.
mod budgets;
mod cancellation;
mod config;
mod engine;
mod publication;

use super::{ReasoningRoute, dependencies::TaskDependency};
use arcs::core::VersionId;
use serde::{Deserialize, Serialize};

pub const PLAN_CANDIDATE: &str = "arcs.plan_candidate.v1";
pub const ACCEPTED_PLAN: &str = "arcs.plan.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningConfig {
    pub max_steps: usize,
    pub max_total_runtime_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_output_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_actions: Option<usize>,
    pub templates: Vec<StepTemplate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepTemplate {
    pub route: ReasoningRoute,
    pub export_fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanProposal {
    pub steps: Vec<ProposedStep>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedStep {
    pub id: String,
    pub template: String,
    #[serde(default)]
    pub dependencies: Vec<StepDependency>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDependency {
    pub step_id: String,
    pub payload_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedPlan {
    pub runtime_id: String,
    pub parent_sequence: u64,
    pub parent_route_id: String,
    pub input_version_id: VersionId,
    pub proposal_version_id: VersionId,
    pub steps: Vec<AcceptedStep>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedStep {
    pub id: String,
    pub template_id: String,
    pub route: ReasoningRoute,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    Running,
    ReadyForReview,
    Rejected,
    Blocked,
    Cancelled,
}

#[derive(Debug, Serialize)]
pub struct PlanReceipt {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub proposal_version_id: Option<VersionId>,
    pub plan_version_id: Option<VersionId>,
    pub state: PlanState,
    pub detail: Option<String>,
    pub cancelled_at_unix_ms: Option<u64>,
    pub steps: Vec<PlanStepReport>,
}
#[derive(Debug, Serialize)]
pub struct PlanStepReport {
    pub sequence: u64,
    pub id: String,
    pub template_id: String,
    pub route_id: String,
    pub state: String,
}

/// Planner-Root vor der Aufnahme eines Vorschlags, auch in Terminpausen.
#[derive(Debug, Serialize)]
pub struct PlannerJob {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub state: String,
}

fn route_id(parent: u64, id: &str) -> String {
    format!("p{parent}.{id}")
}

fn dependency(parent: u64, proposed: &StepDependency) -> TaskDependency {
    TaskDependency {
        route_id: route_id(parent, &proposed.step_id),
        payload_fields: proposed.payload_fields.clone(),
    }
}

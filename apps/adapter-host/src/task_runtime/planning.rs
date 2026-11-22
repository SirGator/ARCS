//! Begrenzte Reasoning-Pläne aus Betreiber-Vorlagen; keine neuen Berechtigungen.
mod availability;
mod budgets;
mod cancellation;
mod config;
mod engine;
mod publication;
mod replanning;

use super::{
    ReasoningRoute,
    dependencies::{DependencyBinding, TaskDependency},
};
use arcs::{
    adapters::CapabilityRef,
    core::{SchemaId, VersionId},
};
use serde::{Deserialize, Serialize};

pub const PLAN_CANDIDATE: &str = "arcs.plan_candidate.v1";
pub const REUSE_PLAN_CANDIDATE: &str = "arcs.plan_candidate.v2";
pub const FAILURE_PLAN_CANDIDATE: &str = "arcs.plan_candidate.v3";
pub const ACCEPTED_PLAN: &str = "arcs.plan.v1";
pub const VERSIONED_PLAN: &str = "arcs.plan.v2";
pub const PLAN_WITH_PREVIOUS_DEPENDENCIES: &str = "arcs.plan.v3";
pub const PLAN_WITH_FAILURE_DEPENDENCIES: &str = "arcs.plan.v4";
pub const REPLANNING_CONTEXT_WITH_FAILURES: &str = "arcs.plan_replanning.v2";
pub const REPLANNING_CONTEXT: &str = "arcs.plan_replanning.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplanningPolicy {
    pub max_revisions: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reuse_completed_exports: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reuse_failure_exports: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRevision {
    pub index: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_plan_version_id: Option<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_version_id: Option<VersionId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplanningContext {
    pub runtime_id: String,
    pub parent_sequence: u64,
    pub input_version_id: VersionId,
    pub revision: u8,
    pub previous_plan_version_id: VersionId,
    pub previous_proposal_version_id: VersionId,
    pub steps: Vec<ReplanningStep>,
    pub context: Vec<arcs::reasoning::ContextSelection>,
    pub omitted_output_items: usize,
}
impl ReplanningContext {
    fn has_failure_evidence(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.failure_receipt_version_id.is_some())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplanningStep {
    pub sequence: u64,
    pub id: String,
    pub template_id: String,
    pub state: String,
    pub resolution: String,
    pub output_version_ids: Vec<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_version_id: Option<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_receipt_version_id: Option<VersionId>,
}

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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_template_availability: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replanning: Option<ReplanningPolicy>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepTemplate {
    pub route: ReasoningRoute,
    pub export_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replanning_failure_receipt_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<PlanOutputPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanOutputPolicy {
    pub destination: CapabilityRef,
    pub receipt_schema_id: SchemaId,
    #[serde(default, skip_serializing_if = "is_first_result")]
    pub result_index: usize,
}

fn is_first_result(index: &usize) -> bool {
    *index == 0
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub previous_revision: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub previous_failure: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<PlanRevision>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedStep {
    pub id: String,
    pub template_id: String,
    pub route: ReasoningRoute,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_dependencies: Vec<DependencyBinding>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    Running,
    Replanning,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revisions: Vec<PlanRevisionReceipt>,
}
#[derive(Debug, Serialize)]
pub struct PlanRevisionReceipt {
    pub index: u8,
    pub plan_version_id: VersionId,
    pub proposal_version_id: VersionId,
    pub context_version_id: Option<VersionId>,
}

#[derive(Debug, Serialize)]
pub struct PlanStepReport {
    pub sequence: u64,
    pub id: String,
    pub template_id: String,
    pub route_id: String,
    pub state: String,
    pub revision: u8,
}

/// Planner-Root vor der Aufnahme eines Vorschlags, auch in Terminpausen.
#[derive(Debug, Serialize)]
pub struct PlannerJob {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub state: String,
}

fn route_id(parent: u64, id: &str, revision: Option<u8>) -> String {
    match revision {
        None => format!("p{parent}.{id}"),
        Some(revision) => format!("p{parent}.r{revision}.{id}"),
    }
}

fn dependency(parent: u64, proposed: &StepDependency, revision: Option<u8>) -> TaskDependency {
    TaskDependency {
        route_id: route_id(parent, &proposed.step_id, revision),
        payload_fields: proposed.payload_fields.clone(),
    }
}

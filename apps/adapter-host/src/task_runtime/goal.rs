//! Fachneutrale Aufgabensteuerung. Pläne und Wirkungsbewertungen kommen von
//! externen Reasonern; Autorität folgt ausschließlich der Betreiberpolicy.
mod engine;
mod gates;

use std::collections::{BTreeMap, HashSet};

use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef};
use arcs::core::{SchemaId, VersionId};
use arcs::outcome::OutcomeVerdict;
use arcs::reasoning::{ReasoningMethod, ReasoningResources};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ReasoningRoute, config::invalid};
use crate::{Host, HostError};

pub const ASSESSMENT_SCHEMA: &str = "arcs.goal_assessment_candidate.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalConfig {
    #[serde(default, skip_serializing_if = "is_false")]
    pub initial_assessment: bool,
    pub max_rounds: u32,
    pub max_wait_ticks: u32,
    pub sources: Vec<GoalSource>,
    pub actions: Vec<ActionRule>,
    #[serde(default)]
    pub approval: GoalApproval,
    pub assessment: AssessmentReasoner,
    pub execution_payload_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalSource {
    pub id: String,
    pub capability: CapabilityRef,
    pub schema_id: SchemaId,
    pub payload_fields: Vec<String>,
    /// None: frische Observe-Ereignisse; Some: separate, persistente Abfrage.
    pub query: Option<GoalQuery>,
    /// Optionaler exakter Core-Subject-Filter für Beobachtungen.
    pub subject_id: Option<String>,
    pub binary: Option<GoalBinary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalQuery {
    pub schema_id: SchemaId,
    pub payload: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalBinary {
    pub name: String,
    pub source: CapabilityRef,
    pub reference_pointer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRule {
    pub capability: CapabilityRef,
    /// Zusätzlicher, vom Betreiber registrierter Payload-Vertrag für die
    /// Verification (z.B. erlaubte Zielgeräte, Wertebereiche, Pflichtfelder).
    pub verification_schema_id: SchemaId,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalApproval {
    #[default]
    Manual,
    Policy {
        policy_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentReasoner {
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub objective: String,
    pub constraints: Value,
    pub budget: ReasoningResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    Acquiring,
    Checking,
    Planning,
    Verifying,
    AwaitingApproval,
    Executing,
    Observing,
    Assessing,
    Completed,
    Blocked,
    Rejected,
    BudgetExhausted,
    Cancelled,
}
impl GoalState {
    pub(super) fn queue_state(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Rejected => "rejected",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::AwaitingApproval => "awaiting_approval",
            _ => "running",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Blocked
                | Self::Rejected
                | Self::BudgetExhausted
                | Self::Cancelled
        )
    }
}

/// Dauerhafter Stand einer einzelnen Aufgabe. Referenzen zeigen auf die
/// unveränderten Core-Artefakte, nie auf vom Reasoner behauptete Versionen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalTask {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub state: GoalState,
    pub round: u32,
    pub observation_cursor: u64,
    pub dispatch_cursor: Option<u64>,
    pub wait_ticks: u32,
    pub source_cursors: BTreeMap<String, u64>,
    pub source_versions: BTreeMap<String, VersionId>,
    pub before: Vec<VersionId>,
    pub after: Vec<VersionId>,
    pub candidate: Option<VersionId>,
    pub capability: Option<CapabilityRef>,
    pub verification: Option<VersionId>,
    pub approval: Option<VersionId>,
    pub action: Option<VersionId>,
    pub execution: Option<VersionId>,
    pub assessment: Option<VersionId>,
    pub outcome: Option<VersionId>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerdict {
    Achieved,
    Continue,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalAssessment {
    pub goal_status: GoalVerdict,
    pub effect: OutcomeVerdict,
    pub detail: String,
    pub evidence_version_ids: Vec<VersionId>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn is_false(value: &bool) -> bool {
    !value
}
fn fields(values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 64
        && values.iter().all(|v| !v.trim().is_empty())
        && values.iter().collect::<HashSet<_>>().len() == values.len()
}

impl GoalConfig {
    pub(super) fn validate(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        manifests: &mut BTreeMap<String, AdapterManifest>,
    ) -> Result<(), HostError> {
        if !(1..=1000).contains(&self.max_rounds)
            || !(1..=10_000).contains(&self.max_wait_ticks)
            || !(1..=16).contains(&self.sources.len())
            || !(1..=32).contains(&self.actions.len())
            || !fields(&self.execution_payload_fields)
            || route.budget.max_candidates != 1
            || route.budget.max_context_items < 2 + self.sources.len()
            || self.assessment.budget.max_context_items < 3 + 2 * self.sources.len()
        {
            return Err(invalid(
                "goals need bounded rounds, waits, sources, actions, sufficient context and exactly one planner candidate",
            ));
        }
        if let GoalApproval::Policy { policy_id } = &self.approval
            && !identifier(policy_id)
        {
            return Err(invalid("invalid approval policy ID"));
        }
        let mut contract = |cap: &CapabilityRef| -> Result<CapabilityContract, HostError> {
            let (registered, descriptor) = host
                .registry()
                .authorized_capability(&cap.adapter_id, &cap.capability_id)
                .map_err(HostError::Registration)?;
            manifests.insert(cap.adapter_id.0.clone(), registered.manifest().clone());
            Ok(descriptor.contract.clone())
        };
        let mut ids = HashSet::new();
        let mut binary_names = HashSet::new();
        for source in &self.sources {
            if !identifier(&source.id) || !ids.insert(&source.id) || !fields(&source.payload_fields)
            {
                return Err(invalid("invalid or duplicate goal source"));
            }
            match (contract(&source.capability)?, &source.query) {
                (CapabilityContract::Request { accepts, emits }, Some(query))
                    if accepts.contains(&query.schema_id) && emits.contains(&source.schema_id) =>
                {
                    host.schemas()
                        .validate(&query.schema_id, &query.payload)
                        .map_err(|e| invalid(format!("invalid source query: {e:?}")))?;
                    if query.payload.to_string().len() > 60_000 {
                        return Err(invalid("goal query too large"));
                    }
                }
                (CapabilityContract::Observe { emits }, None)
                    if emits.contains(&source.schema_id) => {}
                _ => {
                    return Err(invalid(
                        "goal source must match its Request or Observe contract",
                    ));
                }
            }
            if let Some(binary) = &source.binary {
                contract(&binary.source)?;
                if !identifier(&binary.name)
                    || !binary_names.insert(&binary.name)
                    || !binary.reference_pointer.starts_with('/')
                {
                    return Err(invalid("invalid goal binary selection"));
                }
            }
        }
        let mut actions = HashSet::new();
        for action in &self.actions {
            if !actions.insert(&action.capability)
                || !route.allowed_capabilities.contains(&action.capability)
            {
                return Err(invalid(
                    "goal action must be unique and allowed to the planner",
                ));
            }
            if !matches!(contract(&action.capability)?, CapabilityContract::Act {accepts,idempotent:true,..} if accepts.contains(&route.target_schema_id))
            {
                return Err(invalid(
                    "goal action must be idempotent and accept planner candidates",
                ));
            }
            let schema = host
                .schemas()
                .get(&action.verification_schema_id)
                .ok_or_else(|| invalid("missing action verification schema"))?;
            if schema.document.get("type").and_then(Value::as_str) != Some("object")
                || schema.document.get("additionalProperties") != Some(&Value::Bool(false))
                || schema
                    .document
                    .get("required")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            {
                return Err(invalid(
                    "action verification requires a closed object schema with required fields",
                ));
            }
        }
        let assessment = &self.assessment;
        if !matches!(contract(&assessment.capability)?, CapabilityContract::Reason {emits} if emits.contains(&SchemaId(ASSESSMENT_SCHEMA.into())))
            || assessment.objective.trim().is_empty()
            || assessment.objective.len() > 4096
            || assessment.budget.max_candidates != 1
            || !assessment.budget.supports(assessment.method)
        {
            return Err(invalid("invalid assessment reasoner contract or budget"));
        }
        let (registered, _) = host
            .registry()
            .authorized_capability(
                &assessment.capability.adapter_id,
                &assessment.capability.capability_id,
            )
            .map_err(HostError::Registration)?;
        let limits = registered
            .grant()
            .reasoning_limits_v2
            .as_ref()
            .ok_or_else(|| invalid("assessment requires V2 grant"))?;
        if !limits.methods.contains(&assessment.method)
            || !assessment.budget.fits_within(&limits.resources)
        {
            return Err(invalid("assessment exceeds reasoner grant"));
        }
        Ok(())
    }
}

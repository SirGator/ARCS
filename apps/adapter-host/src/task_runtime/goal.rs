//! Fachneutrale Aufgabensteuerung. Pläne und Wirkungsbewertungen kommen von
//! externen Reasonern; Autorität folgt ausschließlich der Betreiberpolicy.
mod acquisition;
mod engine;
mod gates;
mod progress;
mod waiting;

use std::collections::{BTreeMap, HashSet};

use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef};
use arcs::core::{SchemaId, VersionId};
use arcs::outcome::OutcomeVerdict;
use arcs::reasoning::{ReasoningMethod, ReasoningResources};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ReasoningRoute, config::invalid};
use crate::{Host, HostError};

pub(super) const ROUND_SCHEMA: &str = "arcs.goal_round.v1";
pub(super) const ROUND_CAPABILITY: &str = "goal.round.record";

pub const ASSESSMENT_SCHEMA: &str = "arcs.goal_assessment_candidate.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub initial_assessment: bool,
    /// Betreiberdefinierte Wartebedingung nach einer abgeschlossenen Runde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub between_rounds: Option<super::scheduler::Activation>,
    pub max_rounds: u32,
    pub max_wait_ticks: u32,
    pub sources: Vec<GoalSource>,
    pub actions: Vec<ActionRule>,
    #[serde(default)]
    pub approval: GoalApproval,
    pub assessment: AssessmentReasoner,
    pub execution_payload_fields: Vec<String>,
    /// Operator-defined predicate over the committed execution receipt.
    /// Required when the receipt is the sole post-action evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_success_schema_id: Option<SchemaId>,
    /// Export the committed receipt after successful assessment. Legacy goals
    /// export the assessment itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_execution_schema_id: Option<SchemaId>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub from_dependencies: BTreeMap<String, QueryField>,
    /// Additional or substituted fields for the query after a committed action.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub after_from_execution: BTreeMap<String, QueryField>,
}

impl GoalQuery {
    fn validate(&self, host: &Host, execution_fields: &[String]) -> Result<(), HostError> {
        host.schemas()
            .validate(&self.schema_id, &self.payload)
            .map_err(|e| invalid(format!("invalid source query: {e:?}")))?;
        if self.from_dependencies.len() > 16
            || self.from_dependencies.iter().any(|(target, binding)| {
                self.payload.get(target).is_none()
                    || binding.pointer.len() > 512
                    || binding.root_field().is_none()
                    || host.schemas().get(&binding.schema_id).is_none()
            })
        {
            return Err(invalid("goal query has invalid dependency bindings"));
        }
        let query_schema = host
            .schemas()
            .get(&self.schema_id)
            .ok_or_else(|| invalid("missing goal query schema"))?;
        if self.after_from_execution.len() > 16
            || (!self.after_from_execution.is_empty() && !self.payload.is_object())
            || self.after_from_execution.iter().any(|(target, binding)| {
                let Some(root) = binding.root_field() else {
                    return true;
                };
                query_schema.document["properties"].get(target).is_none()
                    || self.from_dependencies.contains_key(target)
                    || binding.pointer.len() > 512
                    || !execution_fields.iter().any(|field| field == root)
                    || host
                        .schemas()
                        .get(&binding.schema_id)
                        .is_none_or(|schema| schema.document["properties"].get(root).is_none())
            })
        {
            return Err(invalid(
                "goal query has invalid after-action execution bindings",
            ));
        }
        if self.payload.to_string().len() > 60_000 {
            return Err(invalid("goal query too large"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryField {
    pub schema_id: SchemaId,
    pub pointer: String,
}

impl QueryField {
    fn root_field(&self) -> Option<&str> {
        exported_root(&self.pointer)
    }
}

fn exported_root(pointer: &str) -> Option<&str> {
    pointer
        .strip_prefix('/')?
        .split('/')
        .next()
        .filter(|name| !name.is_empty() && !name.contains('~'))
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
    /// The resource revision to carry for a controller that requires fencing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fenced_resource: Option<String>,
    /// Match candidate payload values against the frozen pre-action sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_matches: Vec<ActionSourceMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSourceMatch {
    pub source_id: String,
    pub candidate_pointer: String,
    pub source_pointer: String,
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
    WaitingTime,
    WaitingEvent,
    Completed,
    Failed,
    Blocked,
    Rejected,
    BudgetExhausted,
    Cancelled,
}
impl GoalState {
    pub(super) fn queue_state(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Rejected => "rejected",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Cancelled => "cancelled",
            Self::AwaitingApproval => "awaiting_approval",
            Self::WaitingTime | Self::WaitingEvent => "pending",
            _ => "running",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_observation_cursor: Option<u64>,
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
    /// Head of the assessed round history in the Core artifact network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_round_version_id: Option<VersionId>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct GoalReview {
    pub sequence: u64,
    pub candidate_version_id: VersionId,
    pub capability: CapabilityRef,
    pub verification_version_id: VersionId,
    pub payload: Value,
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
        if !self.exclusive_resources.is_empty() {
            arcs::runtime::reservations::validate_resources(&self.exclusive_resources)
                .map_err(|e| invalid(format!("exclusive resources: {e:?}")))?;
        }
        if !(1..=1000).contains(&self.max_rounds)
            || !(1..=10_000).contains(&self.max_wait_ticks)
            || self.sources.len() > 16
            || !(1..=32).contains(&self.actions.len())
            || !fields(&self.execution_payload_fields)
            || route.permitted_reasoner_choices().iter().any(|choice| {
                choice.budget.max_candidates != 1
                    || choice.budget.max_context_items < 2 + self.sources.len()
            })
            || self
                .assessment
                .constraints
                .get(arcs::reasoning::selection::CONSTRAINT)
                .is_some()
            || self
                .assessment
                .constraints
                .get(arcs::reasoning::recovery::CONSTRAINT)
                .is_some()
            || self.assessment.budget.max_context_items < 3 + 2 * self.sources.len()
        {
            return Err(invalid(
                "goals need bounded rounds, waits, sources, actions, sufficient context and exactly one planner candidate",
            ));
        }
        if self.sources.is_empty()
            && (self.execution_success_schema_id.is_none() || self.initial_assessment)
        {
            return Err(invalid(
                "goals without sources require an execution success schema and assessment after execution",
            ));
        }
        if self.export_execution_schema_id.is_some()
            && (self.initial_assessment || self.execution_success_schema_id.is_none())
        {
            return Err(invalid(
                "exporting an execution receipt requires a post-action success schema and no initial assessment",
            ));
        }
        if let Some(schema_id) = &self.execution_success_schema_id {
            validate_required_object(host, schema_id, "execution success")?;
        }
        if let GoalApproval::Policy { policy_id } = &self.approval
            && !identifier(policy_id)
        {
            return Err(invalid("invalid approval policy ID"));
        }
        if let Some(activation) = &self.between_rounds {
            activation.validate(host, manifests)?;
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
                    query.validate(host, &self.execution_payload_fields)?;
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
        let binary_objects = self
            .sources
            .iter()
            .filter(|source| source.binary.is_some())
            .count();
        if (route.reasoner_selection.is_some() || route.reasoning_recovery.is_some())
            && binary_objects > 0
        {
            for choice in route.permitted_reasoner_choices() {
                let (adapter, _) = host
                    .registry()
                    .authorized_capability(
                        &choice.capability.adapter_id,
                        &choice.capability.capability_id,
                    )
                    .map_err(HostError::Registration)?;
                if adapter
                    .grant()
                    .input_data_limits
                    .as_ref()
                    .is_none_or(|limits| limits.max_objects < binary_objects)
                {
                    return Err(invalid(
                        "every selectable goal planner needs a sufficient binary input receiver grant",
                    ));
                }
            }
        }
        for action in &self.actions {
            if !actions.insert(&action.capability)
                || !route.allowed_capabilities.contains(&action.capability)
            {
                return Err(invalid(
                    "goal action must be unique and allowed to the planner",
                ));
            }
            let capability = contract(&action.capability)?;
            let Some(act) = capability.act() else {
                return Err(invalid(
                    "goal action must be idempotent and accept planner candidates",
                ));
            };
            if !act.idempotent || !act.accepts.contains(&route.target_schema_id) {
                return Err(invalid(
                    "goal action must be idempotent and accept planner candidates",
                ));
            }
            if let Some(schema) = &self.export_execution_schema_id
                && act.emits != std::slice::from_ref(schema)
            {
                return Err(invalid(
                    "each goal action must emit the configured execution export schema",
                ));
            }
            if self
                .sources
                .iter()
                .filter_map(|source| source.query.as_ref())
                .flat_map(|query| query.after_from_execution.values())
                .any(|binding| act.emits != std::slice::from_ref(&binding.schema_id))
            {
                return Err(invalid(
                    "each goal action must emit the schema bound to its after-action queries",
                ));
            }
            if act.fenced_resource != action.fenced_resource.as_deref()
                || action
                    .fenced_resource
                    .as_ref()
                    .is_some_and(|resource| !self.exclusive_resources.contains(resource))
            {
                return Err(invalid(
                    "goal action requires a matching fenced resource in exclusive_resources",
                ));
            }
            validate_required_object(host, &action.verification_schema_id, "action verification")?;
            if !action.source_matches.is_empty() {
                self.validate_action_source_matches(host, action)?;
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

    fn validate_action_source_matches(
        &self,
        host: &Host,
        action: &ActionRule,
    ) -> Result<(), HostError> {
        let schema = host
            .schemas()
            .get(&action.verification_schema_id)
            .ok_or_else(|| invalid("missing action verification schema"))?;
        if action.source_matches.len() > 16
            || action.source_matches.iter().any(|rule| {
                let candidate = exported_root(&rule.candidate_pointer);
                let source = exported_root(&rule.source_pointer);
                rule.candidate_pointer.len() > 512
                    || rule.source_pointer.len() > 512
                    || !candidate
                        .is_some_and(|key| schema.document["properties"].get(key).is_some())
                    || !self.sources.iter().any(|configured| {
                        configured.id == rule.source_id
                            && source.is_some_and(|key| {
                                configured.payload_fields.iter().any(|field| field == key)
                            })
                    })
            })
        {
            return Err(invalid(
                "action source matches require declared candidate pointers and exported fields of known goal sources",
            ));
        }
        Ok(())
    }
}

fn validate_required_object(
    host: &Host,
    schema_id: &SchemaId,
    purpose: &str,
) -> Result<(), HostError> {
    let schema = host
        .schemas()
        .get(schema_id)
        .ok_or_else(|| invalid(format!("missing {purpose} schema")))?;
    if schema.document.get("type").and_then(Value::as_str) != Some("object")
        || schema.document.get("additionalProperties") != Some(&Value::Bool(false))
        || schema
            .document
            .get("required")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(invalid(format!(
            "{purpose} requires a closed object schema with required fields"
        )));
    }
    Ok(())
}

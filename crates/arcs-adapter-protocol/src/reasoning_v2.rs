//! Methodenunabhängiger Reasoning-Vertrag; die bisherigen V1-DTOs bleiben erhalten.
use crate::{CapabilityRef, ContextSelection, ProposalSubmission, ReasoningContextItem, SchemaId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMethod {
    Solver,
    Rules,
    Llm,
    Learned,
    Hybrid,
}
impl ReasoningMethod {
    pub fn uses_model(self) -> bool {
        matches!(self, Self::Llm | Self::Learned | Self::Hybrid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningResources {
    pub max_context_items: usize,
    pub max_context_bytes: usize,
    pub max_output_bytes: usize,
    pub max_candidates: usize,
    pub max_runtime_ms: u64,
    pub max_iterations: Option<u64>,
    pub max_output_tokens: Option<usize>,
}
impl ReasoningResources {
    pub fn valid(&self) -> bool {
        self.max_context_items > 0
            && self.max_context_bytes > 0
            && self.max_output_bytes > 0
            && self.max_candidates > 0
            && (1..=3_600_000).contains(&self.max_runtime_ms)
            && self.max_iterations != Some(0)
            && self.max_output_tokens != Some(0)
    }
    pub fn supports(&self, method: ReasoningMethod) -> bool {
        self.valid()
            && match method {
                ReasoningMethod::Solver | ReasoningMethod::Rules => self.max_iterations.is_some(),
                ReasoningMethod::Llm => self.max_output_tokens.is_some(),
                _ => true,
            }
    }
    pub fn fits_within(&self, limit: &Self) -> bool {
        self.valid()
            && self.max_context_items <= limit.max_context_items
            && self.max_context_bytes <= limit.max_context_bytes
            && self.max_output_bytes <= limit.max_output_bytes
            && self.max_candidates <= limit.max_candidates
            && self.max_runtime_ms <= limit.max_runtime_ms
            && self
                .max_iterations
                .is_none_or(|value| limit.max_iterations.is_some_and(|max| value <= max))
            && self
                .max_output_tokens
                .is_none_or(|value| limit.max_output_tokens.is_some_and(|max| value <= max))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningLimitsV2 {
    pub methods: Vec<ReasoningMethod>,
    pub resources: ReasoningResources,
}
impl ReasoningLimitsV2 {
    pub fn valid(&self) -> bool {
        !self.methods.is_empty()
            && self.resources.valid()
            && self
                .methods
                .iter()
                .all(|method| self.resources.supports(*method))
            && self
                .methods
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.methods.len()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningRequestV2 {
    pub request_id: String,
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub objective: String,
    pub context: Vec<ContextSelection>,
    pub target_schema_id: SchemaId,
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub constraints: Value,
    pub budget: ReasoningResources,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningInvocationV2 {
    pub invocation_id: String,
    pub request_id: String,
    pub capability: CapabilityRef,
    pub method: ReasoningMethod,
    pub objective: String,
    pub context: Vec<ReasoningContextItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_data: Vec<crate::InputDataDescriptor>,
    pub target_schema_id: SchemaId,
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub constraints: Value,
    pub budget: ReasoningResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningStatus {
    Solved,
    NoSolution,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningTraceV2 {
    pub method: ReasoningMethod,
    pub implementation: String,
    pub version: String,
    pub iterations: Option<u64>,
    pub output_tokens: Option<usize>,
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningResponseV2 {
    pub invocation_id: String,
    pub request_id: String,
    pub status: ReasoningStatus,
    pub candidates: Vec<ProposalSubmission>,
    pub trace: ReasoningTraceV2,
}

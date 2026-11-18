//! Deterministic lexical discovery over authorized technical contracts.
//! Relevance is advisory and never a claim of safety, reliability or success.
use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use super::{AdapterRegistry, AdapterRegistryError, AvailableCapability, ProducerClass};
use crate::adapters::{AdapterRole, CapabilityRef};
use crate::core::SchemaId;
use crate::reasoning::ReasoningMethod;

fn limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityQuery {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub role: Option<AdapterRole>,
    #[serde(default)]
    pub accepts_schema: Option<SchemaId>,
    #[serde(default)]
    pub emits_schema: Option<SchemaId>,
    #[serde(default)]
    pub reasoning_method: Option<ReasoningMethod>,
    /// None searches the registered catalog; Some([]) deliberately finds none.
    #[serde(default)]
    pub allowed_capabilities: Option<Vec<CapabilityRef>>,
    #[serde(default = "limit")]
    pub limit: usize,
}

#[derive(Debug)]
pub enum CapabilityDiscoveryError {
    InvalidQuery(String),
    Authorization(AdapterRegistryError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityMatch {
    pub capability: AvailableCapability,
    pub adapter_version: String,
    pub producer_class: ProducerClass,
    pub relevance_score: u32,
    pub matched_terms: Vec<String>,
}

impl CapabilityQuery {
    /// Invalid queries never become a broader search or a model fallback.
    pub fn validate(&self) -> Result<(), CapabilityDiscoveryError> {
        let invalid = |message: &str| CapabilityDiscoveryError::InvalidQuery(message.into());
        if self.text.len() > 4096
            || self.text.chars().any(char::is_control)
            || tokens(&self.text).len() > 32
            || !(1..=64).contains(&self.limit)
            || self.reasoning_method.is_some()
                && self.role.is_some_and(|role| role != AdapterRole::Reason)
        {
            return Err(invalid(
                "invalid search text, method/role combination or result limit",
            ));
        }
        if [&self.accepts_schema, &self.emits_schema]
            .into_iter()
            .flatten()
            .any(|schema| {
                schema.0.trim().is_empty()
                    || schema.0.len() > 256
                    || schema.0.chars().any(char::is_control)
            })
        {
            return Err(invalid("schema filters must contain 1..256 bytes"));
        }
        if let Some(scope) = &self.allowed_capabilities
            && (scope.len() > 256
                || scope.iter().collect::<HashSet<_>>().len() != scope.len()
                || scope.iter().any(|cap| {
                    [&cap.adapter_id.0, &cap.capability_id.0]
                        .into_iter()
                        .any(|id| {
                            id.is_empty()
                                || id.len() > 128
                                || !id
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
                        })
                }))
        {
            return Err(invalid(
                "invalid or duplicate capability scope; at most 256 entries",
            ));
        }
        if !self.text.trim().is_empty() && tokens(&self.text).is_empty() {
            return Err(invalid("search text contains no words"));
        }
        Ok(())
    }
}

impl AdapterRegistry {
    pub fn find_capabilities(
        &self,
        query: &CapabilityQuery,
    ) -> Result<Vec<CapabilityMatch>, CapabilityDiscoveryError> {
        self.find_capabilities_with(query, |_| true)
    }

    /// A host may further restrict candidates by connected/known unavailable
    /// installations. This predicate is applied before ranking and truncation;
    /// it cannot add any capability outside the registry's grants or query scope.
    pub fn find_capabilities_with(
        &self,
        query: &CapabilityQuery,
        eligible: impl Fn(&CapabilityRef) -> bool,
    ) -> Result<Vec<CapabilityMatch>, CapabilityDiscoveryError> {
        query.validate()?;
        let scope = query
            .allowed_capabilities
            .as_ref()
            .map(|scope| scope.iter().collect::<HashSet<_>>());
        if let Some(scope) = &query.allowed_capabilities {
            for capability in scope {
                self.authorized_capability(&capability.adapter_id, &capability.capability_id)
                    .map_err(CapabilityDiscoveryError::Authorization)?;
            }
        }
        let terms = tokens(&query.text);
        let mut matches = Vec::new();
        for connection in self.catalog() {
            for capability in connection.capabilities {
                if !eligible(&capability.capability)
                    || scope
                        .as_ref()
                        .is_some_and(|scope| !scope.contains(&capability.capability))
                    || query.role.is_some_and(|role| role != capability.role)
                    || query
                        .accepts_schema
                        .as_ref()
                        .is_some_and(|schema| !capability.accepts.contains(schema))
                    || query
                        .emits_schema
                        .as_ref()
                        .is_some_and(|schema| !capability.emits.contains(schema))
                    || query.reasoning_method.is_some_and(|method| {
                        capability.role != AdapterRole::Reason
                            || !capability.reasoning_methods.contains(&method)
                    })
                {
                    continue;
                }
                // Catalog entries already come from enabled grants. Recheck the
                // concrete contract so discovery never bypasses authorization.
                self.authorized_capability(
                    &capability.capability.adapter_id,
                    &capability.capability.capability_id,
                )
                .map_err(CapabilityDiscoveryError::Authorization)?;
                let Some(score) = relevance(&capability, &terms) else {
                    continue;
                };
                matches.push(CapabilityMatch {
                    capability,
                    adapter_version: connection.adapter_version.clone(),
                    producer_class: connection.producer_class,
                    relevance_score: score,
                    matched_terms: terms.iter().cloned().collect(),
                });
            }
        }
        matches.sort_by(|a, b| {
            b.relevance_score
                .cmp(&a.relevance_score)
                .then_with(|| a.capability.capability.cmp(&b.capability.capability))
        });
        matches.truncate(query.limit);
        Ok(matches)
    }
}

fn tokens(value: &str) -> BTreeSet<String> {
    value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn relevance(capability: &AvailableCapability, terms: &BTreeSet<String>) -> Option<u32> {
    let ids = tokens(&format!(
        "{} {}",
        capability.capability.adapter_id.0, capability.capability.capability_id.0
    ));
    let mut purpose = BTreeSet::new();
    let mut tags = BTreeSet::new();
    let mut other = BTreeSet::new();
    if let Some(claim) = &capability.description_claim {
        let description = &claim.description;
        purpose = tokens(&description.purpose);
        tags = description
            .tags
            .iter()
            .flat_map(|tag| tokens(tag))
            .collect();
        other = [
            &description.preconditions,
            &description.effects,
            &description.constraints,
            &description.resources,
        ]
        .into_iter()
        .flatten()
        .flat_map(|item| tokens(item))
        .collect();
    }
    let mut score = 0;
    for term in terms {
        let value = if tags.contains(term) {
            4
        } else if purpose.contains(term) {
            2
        } else if ids.contains(term) || other.contains(term) {
            1
        } else {
            return None;
        };
        score += value;
    }
    Some(score)
}

#[cfg(test)]
mod tests;

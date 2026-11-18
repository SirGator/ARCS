//! Per-task Reason bindings. Selection is an existing Core/Reason concern;
//! task orchestration only binds its operator policy and immutable input.
use super::{ReasoningRoute, TaskRuntime, config::invalid};
use crate::{Host, HostError};
use arcs::adapters::AdapterManifest;
use arcs::core::{Artifact, VersionId};
use arcs::reasoning::selection::{
    ReasonerChoice, SelectedReasoner, SelectionOwner, SelectionPolicy, SelectionPreparation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonerRouting {
    pub alternatives: Vec<ReasonerChoice>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub search_text: String,
    /// Explicit Core-only input fields; values are not exported to the reasoner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_input_fields: Vec<String>,
}

impl ReasonerRouting {
    pub(super) fn validate(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        manifests: &mut BTreeMap<String, AdapterManifest>,
    ) -> Result<(), HostError> {
        let mut fields = HashSet::new();
        if self.alternatives.is_empty()
            || self.alternatives.len() > 7
            || self.search_input_fields.len() > 16
            || self.search_input_fields.iter().any(|field| {
                field.trim().is_empty()
                    || field.len() > 128
                    || field.chars().any(char::is_control)
                    || !fields.insert(field)
            })
        {
            return Err(invalid(
                "reasoner selection needs 1..7 alternatives and at most 16 distinct query fields",
            ));
        }
        let policy = SelectionPolicy {
            target_schema_id: route.target_schema_id.clone(),
            query_text: self.search_text.clone(),
            choices: route.reasoner_choices(),
        };
        policy
            .validate(host.registry(), host.schemas())
            .map_err(HostError::Reasoning)?;
        for choice in &self.alternatives {
            let (adapter, _) = host
                .registry()
                .authorized_capability(
                    &choice.capability.adapter_id,
                    &choice.capability.capability_id,
                )
                .map_err(HostError::Registration)?;
            manifests.insert(
                adapter.manifest().adapter_id.0.clone(),
                adapter.manifest().clone(),
            );
        }
        Ok(())
    }
}

impl ReasoningRoute {
    pub(in crate::task_runtime) fn reasoner_choices(&self) -> Vec<ReasonerChoice> {
        std::iter::once(ReasonerChoice {
            capability: self.reasoner.clone(),
            method: self.method,
            budget: self.budget.clone(),
        })
        .chain(
            self.reasoner_selection
                .iter()
                .flat_map(|routing| routing.alternatives.iter().cloned()),
        )
        .collect()
    }
}

#[derive(Debug, Serialize)]
pub struct ReasonerSelectionReport {
    pub sequence: u64,
    pub route_id: String,
    pub selection_version_id: VersionId,
    pub selected: ReasonerChoice,
    pub relevance_score: u32,
}

impl TaskRuntime {
    fn selection_owner(
        &self,
        sequence: u64,
        input_version: VersionId,
    ) -> Result<SelectionOwner, HostError> {
        Ok(SelectionOwner {
            definition_version: self.journal.definition_version()?,
            input_version,
            task_id: sequence.to_string(),
        })
    }
    fn selection_identity(
        &self,
        host: &Host,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
    ) -> Result<(SelectionOwner, SelectionPolicy), HostError> {
        let routing = route
            .reasoner_selection
            .as_ref()
            .ok_or_else(|| invalid("route has no selection policy"))?;
        let mut query = routing.search_text.clone();
        for field in &routing.search_input_fields {
            let value = input
                .payload
                .get(field)
                .and_then(serde_json::Value::as_str)
                .filter(|value| value.chars().any(char::is_alphanumeric))
                .ok_or_else(|| invalid("selection query field must be an existing input string"))?;
            if !query.is_empty() {
                query.push(' ');
            }
            if query.len().saturating_add(value.len()) > 4096 {
                return Err(invalid("selection query exceeds 4096 bytes"));
            }
            query.push_str(value);
        }
        let owner = SelectionOwner {
            definition_version: self.journal.definition_version()?,
            input_version: input.version_id.clone(),
            task_id: sequence.to_string(),
        };
        let policy = SelectionPolicy {
            target_schema_id: route.target_schema_id.clone(),
            query_text: query,
            choices: route.reasoner_choices(),
        };
        policy
            .validate(host.registry(), host.schemas())
            .map_err(HostError::Reasoning)?;
        Ok((owner, policy))
    }

    pub(super) fn bind_reasoner_route(
        &self,
        host: &mut Host,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
    ) -> Result<ReasoningRoute, HostError> {
        if route.reasoner_selection.is_none() {
            return Ok(route.clone());
        }
        let (owner, policy) = self.selection_identity(host, sequence, route, input)?;
        match host.select_reasoner(&owner, &policy)? {
            SelectionPreparation::Selected(selection) => Ok(apply(route.clone(), &selection)),
            SelectionPreparation::Waiting => Err(HostError::ResourceBusy {
                resource: "reasoner-selection".into(),
                requested: 1,
                available: 0,
            }),
        }
    }

    pub(in crate::task_runtime) fn apply_saved_reasoner(
        &self,
        host: &Host,
        sequence: u64,
        route: ReasoningRoute,
    ) -> Result<ReasoningRoute, HostError> {
        if route.reasoner_selection.is_none() {
            return Ok(route);
        }
        let (_, input) = self.journal.selection_job(sequence)?;
        if !host.has_reasoner_selection(&self.selection_owner(sequence, input.clone())?)? {
            return Ok(route);
        }
        let input = host
            .store()
            .get(&input)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("selection input is missing"))?;
        let (owner, policy) = self.selection_identity(host, sequence, &route, &input)?;
        match host.load_reasoner_selection(&owner, &policy)? {
            Some(selection) => Ok(apply(route, &selection)),
            None => Ok(route),
        }
    }

    pub(super) fn saved_reasoner_for_task(
        &self,
        host: &Host,
        sequence: u64,
    ) -> Result<Option<SelectedReasoner>, HostError> {
        if !self.has_reasoner_selection {
            return Ok(None);
        }
        let (id, input) = self.journal.selection_job(sequence)?;
        let route = self.raw_route_for_job(host, sequence, &id)?;
        if route.reasoner_selection.is_none() {
            return Ok(None);
        }
        if !host.has_reasoner_selection(&self.selection_owner(sequence, input.clone())?)? {
            return Ok(None);
        }
        let input = host
            .store()
            .get(&input)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("selection input is missing"))?;
        let (owner, policy) = self.selection_identity(host, sequence, &route, &input)?;
        host.load_reasoner_selection(&owner, &policy)
    }

    /// Bounded status view with no private query text or availability snapshot.
    pub fn reasoner_selections(
        &self,
        host: &Host,
    ) -> Result<Vec<ReasonerSelectionReport>, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        if !self.has_reasoner_selection {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        for (sequence, id) in self.journal.selection_report_jobs()? {
            if let Some(selection) = self.saved_reasoner_for_task(host, sequence)? {
                result.push(ReasonerSelectionReport {
                    sequence,
                    route_id: id,
                    selection_version_id: selection.version_id.clone(),
                    selected: selection.choice().clone(),
                    relevance_score: selection.record.observations[selection.record.selected_index]
                        .relevance_score,
                });
            }
        }
        Ok(result)
    }
}

fn apply(mut route: ReasoningRoute, selection: &SelectedReasoner) -> ReasoningRoute {
    let choice = selection.choice();
    route.reasoner = choice.capability.clone();
    route.method = choice.method;
    route.budget = choice.budget.clone();
    route.reasoner_selection = None;
    route
}

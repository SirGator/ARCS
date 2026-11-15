use super::*;
use crate::task_runtime::config::{identifier, invalid, validate_route_contract};
use crate::task_runtime::goal::GoalApproval;
use crate::{Host, HostError};
use arcs::adapters::AdapterManifest;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

impl PlanningConfig {
    pub(in crate::task_runtime) fn validate(
        &self,
        host: &Host,
        owner: &ReasoningRoute,
        manifests: &mut BTreeMap<String, AdapterManifest>,
    ) -> Result<(), HostError> {
        if !(1..=31).contains(&self.max_steps)
            || !(1..=32).contains(&self.templates.len())
            || !(1..=111_600_000).contains(&self.max_total_runtime_ms)
            || self.max_total_output_tokens == Some(0)
            || self.max_total_actions.is_some_and(|n| n > 31_000)
            || owner.goal.is_some()
            || owner.target_schema_id.0 != PLAN_CANDIDATE
            || owner.budget.max_candidates != 1
            || !owner.allowed_capabilities.is_empty()
            || owner.constraints.get("task_planning").is_some()
        {
            return Err(invalid(
                "planning needs a bounded step/catalog budget, plan candidate target, one proposal and no direct action authority",
            ));
        }
        let mut ids = HashSet::new();
        for template in &self.templates {
            let route = &template.route;
            if route.plan.is_some()
                || !route.dependencies.is_empty()
                || (route.goal.is_none() && !route.allowed_capabilities.is_empty())
                || !ids.insert(&route.id)
                || route.source != owner.source
                || route.input_schema_id != owner.input_schema_id
                || template.export_fields.is_empty()
                || template.export_fields.len() > 64
                || template
                    .export_fields
                    .iter()
                    .any(|field| field.trim().is_empty() || field.len() > 128)
                || template.export_fields.iter().collect::<HashSet<_>>().len()
                    != template.export_fields.len()
                || route.constraints.get("task_dependencies").is_some()
            {
                return Err(invalid(
                    "plan templates must be distinct authorized routes for the same input, without nested plans or fixed dependencies, with explicit exports and action authority confined to goals",
                ));
            }
            if let Some(goal) = &route.goal
                && (!matches!(goal.approval, GoalApproval::Manual)
                    || self.max_total_actions.is_none())
            {
                return Err(invalid(
                    "goal templates require manual approval and an explicit aggregate action limit",
                ));
            }
            validate_route_contract(host, route, manifests)?;
        }
        Ok(())
    }

    pub(super) fn catalog(&self) -> Value {
        let mut catalog = json!({"max_steps":self.max_steps,"max_total_runtime_ms":self.max_total_runtime_ms,
            "max_total_output_tokens":self.max_total_output_tokens,
            "templates":self.templates.iter().map(|template| {
                let mut entry = json!({"id":template.route.id,
                "objective":template.route.objective,"reasoner":template.route.reasoner,
                "method":template.route.method,"target_schema_id":template.route.target_schema_id,
                "input_fields":template.route.payload_fields,"export_fields":template.export_fields,
                "budget":template.route.budget});
                if let Some(goal) = &template.route.goal {
                    entry["kind"] = json!("goal");
                    entry["result_schema_id"] = json!(crate::task_runtime::goal::ASSESSMENT_SCHEMA);
                    entry["goal_limits"] = json!({"max_rounds":goal.max_rounds,"initial_assessment":goal.initial_assessment,
                        "approval":"manual","actions":goal.actions.iter().map(|a| &a.capability).collect::<Vec<_>>(),
                        "assessment_method":goal.assessment.method,"assessment_budget":goal.assessment.budget});
                }
                entry
            }).collect::<Vec<_>>()});
        // Keep existing pure Reasoning catalogs byte-for-byte equivalent on
        // recovery. New optional configuration must not change old requests.
        if let Some(limit) = self.max_total_actions {
            catalog["max_total_actions"] = json!(limit);
        }
        catalog
    }

    pub(super) fn compile(
        &self,
        parent: u64,
        proposal: &PlanProposal,
        static_routes: &[ReasoningRoute],
    ) -> Result<Vec<AcceptedStep>, HostError> {
        if proposal.steps.is_empty() || proposal.steps.len() > self.max_steps {
            return Err(invalid("plan step count exceeds configured limit"));
        }
        let templates: HashMap<_, _> = self
            .templates
            .iter()
            .map(|t| (t.route.id.as_str(), t))
            .collect();
        let mut steps = HashMap::new();
        let mut budget = super::budgets::PlanBudget::default();
        for step in &proposal.steps {
            if !identifier(&step.id)
                || step.id.len() > 32
                || steps.insert(step.id.as_str(), step).is_some()
            {
                return Err(invalid("plan needs unique 1..32 byte step identifiers"));
            }
            let template = templates
                .get(step.template.as_str())
                .ok_or_else(|| invalid("plan selects an unknown template"))?;
            budget.add(&template.route, self.max_total_output_tokens.is_some())?;
        }
        budget.check(self)?;
        let mut accepted = Vec::new();
        for step in &proposal.steps {
            let mut route = templates[step.template.as_str()].route.clone();
            route.id = route_id(parent, &step.id);
            if static_routes.iter().any(|fixed| fixed.id == route.id) {
                return Err(invalid("generated plan route collides with a static route"));
            }
            route.dependencies.clear();
            for edge in &step.dependencies {
                let upstream = steps
                    .get(edge.step_id.as_str())
                    .ok_or_else(|| invalid("plan references an unknown step"))?;
                let allowed = &templates[upstream.template.as_str()].export_fields;
                if edge
                    .payload_fields
                    .iter()
                    .any(|field| !allowed.contains(field))
                {
                    return Err(invalid(
                        "plan requests a field outside the template exports",
                    ));
                }
                route.dependencies.push(dependency(parent, edge));
            }
            accepted.push(AcceptedStep {
                id: step.id.clone(),
                template_id: step.template.clone(),
                route,
            });
        }
        let routes: Vec<_> = accepted.iter().map(|s| s.route.clone()).collect();
        crate::task_runtime::dependencies::validate_routes(&routes)?;
        Ok(accepted)
    }
}

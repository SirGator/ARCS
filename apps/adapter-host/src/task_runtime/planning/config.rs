use super::*;
use crate::task_runtime::config::{identifier, invalid, validate_route_contract};
use crate::task_runtime::goal::GoalApproval;
use crate::{Host, HostError};
use arcs::adapters::AdapterManifest;
use arcs::reasoning::ContextSelection;
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
            || ![PLAN_CANDIDATE, REUSE_PLAN_CANDIDATE].contains(&owner.target_schema_id.0.as_str())
            || owner
                .permitted_reasoner_choices()
                .iter()
                .any(|choice| choice.budget.max_candidates != 1)
            || !owner.allowed_capabilities.is_empty()
            || owner.constraints.get("task_planning").is_some()
        {
            return Err(invalid(
                "planning needs a bounded step/catalog budget, plan candidate target, one proposal and no direct action authority",
            ));
        }
        if let Some(policy) = &self.replanning {
            if policy.reuse_completed_exports && owner.target_schema_id.0 != REUSE_PLAN_CANDIDATE {
                return Err(invalid(
                    "reuse_completed_exports requires arcs.plan_candidate.v2",
                ));
            }
            let mut budget = super::budgets::PlanBudget::default();
            budget.add(owner, self.max_total_output_tokens.is_some())?;
            budget.check(self)?;
            if !(1..=7).contains(&policy.max_revisions)
                || owner.constraints.get("task_replanning").is_some()
                || owner
                    .permitted_reasoner_choices()
                    .iter()
                    .any(|c| c.budget.max_context_items < 3)
            {
                return Err(invalid(
                    "replanning needs 1..7 revisions, three context slots and its reserved constraint namespace",
                ));
            }
        }
        let mut ids = HashSet::new();
        for template in &self.templates {
            let route = &template.route;
            if route.plan.is_some()
                || route.recurrence.is_some()
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
            if self.replanning.is_some()
                && route
                    .goal
                    .as_ref()
                    .is_some_and(|g| g.exclusive_resources.is_empty())
            {
                return Err(invalid(
                    "goal replanning requires explicit exclusive control resource identities",
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
            if route.max_reasoning_requests.is_some() {
                return Err(invalid(
                    "max_reasoning_requests belongs to the plan's entry route",
                ));
            }
            validate_route_contract(host, route, manifests)?;
            if let Some(output) = &template.output {
                let limit = if route.goal.is_some() {
                    1
                } else {
                    route
                        .permitted_reasoner_choices()
                        .iter()
                        .map(|choice| choice.budget.max_candidates)
                        .max()
                        .unwrap_or(0)
                };
                if output.result_index >= limit {
                    return Err(invalid(
                        "plan output result_index exceeds the template result budget",
                    ));
                }
                arcs::output::validate_output_contract(
                    host.registry(),
                    host.schemas(),
                    &output.destination,
                    &SchemaId(arcs::output::TASK_REPORT_SCHEMA.into()),
                    &output.receipt_schema_id,
                )
                .map_err(|source| HostError::Output {
                    adapter: output.destination.adapter_id.clone(),
                    source,
                })?;
                let (registered, _) = host
                    .registry()
                    .authorized_capability(
                        &output.destination.adapter_id,
                        &output.destination.capability_id,
                    )
                    .map_err(HostError::Registration)?;
                manifests.insert(
                    output.destination.adapter_id.0.clone(),
                    registered.manifest().clone(),
                );
            }
        }
        Ok(())
    }

    pub(super) fn catalog(&self, host: &Host) -> Result<Value, HostError> {
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
        if let Some(policy) = &self.replanning {
            catalog["replanning"] = json!(policy);
        }
        if let Some(limit) = self.max_total_actions {
            catalog["max_total_actions"] = json!(limit);
        }
        // Claims are static catalog data, not live availability. Changing a
        // running worker's health must never change a frozen planner request.
        let capabilities = host
            .registry()
            .catalog()
            .into_iter()
            .flat_map(|connection| connection.capabilities)
            .map(|cap| (cap.capability.clone(), cap))
            .collect::<BTreeMap<_, _>>();
        for (template, entry) in self.templates.iter().zip(
            catalog["templates"]
                .as_array_mut()
                .expect("template catalog array"),
        ) {
            let capability = capabilities
                .get(&template.route.reasoner)
                .ok_or_else(|| invalid("template reasoner is no longer authorized"))?;
            if let Some(claim) = &capability.description_claim {
                entry["reasoner_description_claim"] = json!(claim);
            }
            if template.route.reasoner_selection.is_some() {
                entry["reasoner_choices"] = json!(
                    template
                        .route
                        .reasoner_choices()
                        .iter()
                        .map(|choice| {
                            let mut value = json!(choice);
                            if let Some(claim) = capabilities
                                .get(&choice.capability)
                                .and_then(|cap| cap.description_claim.as_ref())
                            {
                                value["description_claim"] = json!(claim);
                            }
                            value
                        })
                        .collect::<Vec<_>>()
                );
            }
            if let Some(policy) = &template.route.reasoning_recovery {
                entry["reasoning_recovery"] = json!(policy);
            }
            if let Some(goal) = &template.route.goal {
                let mut actions = Vec::new();
                for action in &goal.actions {
                    let capability = capabilities
                        .get(&action.capability)
                        .ok_or_else(|| invalid("template action is no longer authorized"))?;
                    if let Some(claim) = &capability.description_claim {
                        actions.push(
                            json!({"capability":action.capability,"description_claim":claim}),
                        );
                    }
                }
                if !actions.is_empty() {
                    entry["action_description_claims"] = json!(actions);
                }
            }
        }
        Ok(catalog)
    }

    pub(super) fn compile_revision(
        &self,
        parent: u64,
        proposal: &PlanProposal,
        static_routes: &[ReasoningRoute],
        revision: u8,
        previous: Option<&ReplanningContext>,
    ) -> Result<Vec<AcceptedStep>, HostError> {
        if previous.is_some_and(|context| {
            revision == 0 || context.parent_sequence != parent || context.revision != revision
        }) {
            return Err(invalid(
                "previous dependency context has the wrong plan generation",
            ));
        }
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
        let mut validation_routes = Vec::new();
        let mut previous_routes = HashMap::new();
        for step in &proposal.steps {
            let mut route = templates[step.template.as_str()].route.clone();
            route.id = route_id(parent, &step.id, self.replanning.as_ref().map(|_| revision));
            if static_routes.iter().any(|fixed| fixed.id == route.id) {
                return Err(invalid("generated plan route collides with a static route"));
            }
            route.dependencies.clear();
            let mut previous_dependencies = Vec::new();
            let mut previous_edges = Vec::new();
            for edge in &step.dependencies {
                if edge.previous_revision {
                    if !self
                        .replanning
                        .as_ref()
                        .is_some_and(|policy| policy.reuse_completed_exports)
                    {
                        return Err(invalid(
                            "previous_revision dependencies are not enabled for this plan",
                        ));
                    }
                    let previous = previous.ok_or_else(|| {
                        invalid("previous_revision dependencies require a replacement plan")
                    })?;
                    let (binding, source_route) = previous_dependency(edge, previous, &templates)?;
                    previous_edges.push(TaskDependency {
                        route_id: binding.route_id.clone(),
                        payload_fields: edge.payload_fields.clone(),
                    });
                    previous_routes
                        .entry(source_route.id.clone())
                        .or_insert(source_route);
                    previous_dependencies.push(binding);
                    continue;
                }
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
                route.dependencies.push(dependency(
                    parent,
                    edge,
                    self.replanning.as_ref().map(|_| revision),
                ));
            }
            let mut validation_route = route.clone();
            validation_route.dependencies.extend(previous_edges);
            validation_routes.push(validation_route);
            accepted.push(AcceptedStep {
                id: step.id.clone(),
                template_id: step.template.clone(),
                route,
                previous_dependencies,
            });
        }
        validation_routes.extend(previous_routes.into_values());
        crate::task_runtime::dependencies::validate_routes(&validation_routes)?;
        Ok(accepted)
    }
}

/// A prior step contributes only its versioned exports admitted to the replanner's context.
fn previous_dependency(
    edge: &StepDependency,
    context: &ReplanningContext,
    templates: &HashMap<&str, &StepTemplate>,
) -> Result<(DependencyBinding, ReasoningRoute), HostError> {
    let step = context
        .steps
        .iter()
        .find(|step| step.id == edge.step_id && step.resolution == "ready")
        .ok_or_else(|| invalid("previous_revision must name a completed predecessor step"))?;
    let template = templates
        .get(step.template_id.as_str())
        .ok_or_else(|| invalid("completed predecessor has no authorized template"))?;
    let result_version_id = step
        .result_version_id
        .clone()
        .ok_or_else(|| invalid("completed predecessor result reference is missing"))?;
    if step.output_version_ids.is_empty()
        || edge
            .payload_fields
            .iter()
            .any(|field| !template.export_fields.contains(field))
    {
        return Err(invalid(
            "previous_revision dependency exceeds the predecessor exports",
        ));
    }
    let mut results = Vec::new();
    for version in &step.output_version_ids {
        let selected = context.context.iter().any(|item| {
            &item.version_id == version
                && edge
                    .payload_fields
                    .iter()
                    .all(|field| item.payload_fields.contains(field))
        });
        if !selected {
            return Err(invalid(
                "previous_revision dependency uses an export omitted from the replanning context",
            ));
        }
        results.push(ContextSelection {
            version_id: version.clone(),
            payload_fields: edge.payload_fields.clone(),
        });
    }
    // The source is used only for DAG and context-limit validation. Already
    // committed exports occupy their exact number of context slots.
    let mut source_route = template.route.clone();
    source_route.id = route_id(
        context.parent_sequence,
        &step.id,
        Some(context.revision - 1),
    );
    source_route.budget.max_candidates = results.len();
    Ok((
        DependencyBinding {
            sequence: step.sequence,
            route_id: source_route.id.clone(),
            result_version_id,
            context: results,
        },
        source_route,
    ))
}

use std::collections::{HashMap, HashSet};

use super::TaskDependency;
use crate::HostError;
use crate::task_runtime::{ReasoningRoute, config::invalid};

pub(in crate::task_runtime) fn validate(routes: &[ReasoningRoute]) -> Result<(), HostError> {
    let by_id: HashMap<_, _> = routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect();
    for route in routes {
        if !route.dependencies.is_empty()
            && route.goal.is_none()
            && route.constraints.get("task_dependencies").is_some()
        {
            return Err(invalid(
                "task_dependencies is reserved for runtime-generated result bindings",
            ));
        }
        if route.dependencies.len() > 31 {
            return Err(invalid("at most 31 dependencies are allowed per route"));
        }
        let mut seen = HashSet::new();
        let mut items: usize = 0;
        for TaskDependency {
            route_id,
            payload_fields,
        } in &route.dependencies
        {
            let parent = by_id
                .get(route_id.as_str())
                .ok_or_else(|| invalid("dependency names an unknown route"))?;
            if parent.plan.is_some() {
                return Err(invalid(
                    "plan orchestrators cannot serve as result dependencies; consume their step results through the plan report",
                ));
            }
            if route_id == &route.id
                || !seen.insert(route_id)
                || parent.source != route.source
                || parent.input_schema_id != route.input_schema_id
                || payload_fields.is_empty()
                || payload_fields.len() > 64
                || payload_fields
                    .iter()
                    .any(|field| field.trim().is_empty() || field.len() > 128)
                || payload_fields.iter().collect::<HashSet<_>>().len() != payload_fields.len()
            {
                return Err(invalid(
                    "dependencies need distinct routes for the same Input/Observe capability and schema, with bounded explicit payload fields",
                ));
            }
            let outputs = if parent.goal.is_some() {
                1
            } else {
                parent.budget.max_candidates
            };
            items = items
                .checked_add(outputs)
                .filter(|n| *n <= 10_000)
                .ok_or_else(|| invalid("dependency context reservation is too large"))?;
        }
        if let Some(goal) = &route.goal {
            if route.budget.max_context_items < items + 2 + goal.sources.len()
                || goal.assessment.budget.max_context_items < items + 3 + 2 * goal.sources.len()
            {
                return Err(invalid(
                    "goal context budgets must reserve room for all dependency results",
                ));
            }
        } else if let Some(plan) = route
            .plan
            .as_ref()
            .filter(|plan| plan.include_template_availability)
        {
            let minimum = items + if plan.replanning.is_some() { 4 } else { 2 };
            if route
                .permitted_reasoner_choices()
                .iter()
                .any(|choice| choice.budget.max_context_items < minimum)
            {
                return Err(invalid(format!(
                    "template availability and dependencies need at least {minimum} planner context slots"
                )));
            }
        } else if route.budget.max_context_items < 1 + items {
            return Err(invalid(
                "context budget must reserve room for task input and dependency results",
            ));
        }
    }
    let mut done = HashSet::new();
    let mut active = HashSet::new();
    fn visit<'a>(
        id: &'a str,
        routes: &HashMap<&'a str, &'a ReasoningRoute>,
        active: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), HostError> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err(invalid("cyclic task dependencies are not allowed"));
        }
        for dependency in &routes[id].dependencies {
            visit(&dependency.route_id, routes, active, done)?;
        }
        active.remove(id);
        done.insert(id);
        Ok(())
    }
    for route in routes {
        visit(&route.id, &by_id, &mut active, &mut done)?;
    }
    Ok(())
}

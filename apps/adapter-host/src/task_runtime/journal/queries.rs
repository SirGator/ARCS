//! Operator-defined query publishers for static and instantiated goal routes.
use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{RuntimeConfig, goal::GoalConfig};
use arcs::adapters::{CapabilityContract, CapabilityDescriptor, CapabilityId};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use rusqlite::OptionalExtension;

fn template_query(owner: &str, template: &str, source: &str) -> CapabilityId {
    let key = deterministic_invocation_id(
        InvocationKind::Request,
        &["goal-template-query", owner, template, source],
    );
    CapabilityId(format!("runtime.template.query.{}", key.replace(':', ".")))
}

pub(super) fn query_capabilities(config: &RuntimeConfig) -> Vec<CapabilityDescriptor> {
    let mut capabilities = Vec::new();
    let mut add = |goal: &GoalConfig, id: &dyn Fn(&str) -> CapabilityId| {
        for source in &goal.sources {
            if let Some(query) = &source.query {
                capabilities.push(CapabilityDescriptor {
                    description: None,
                    id: id(&source.id),
                    contract: CapabilityContract::Input {
                        emits: vec![query.schema_id.clone()],
                    },
                    required_permissions: vec![],
                });
            }
        }
    };
    for route in &config.routes {
        if let Some(goal) = &route.goal {
            add(goal, &|source| {
                CapabilityId(format!("runtime.query.{}.{}", route.id, source))
            });
        }
        if let Some(plan) = &route.plan {
            for template in &plan.templates {
                if let Some(goal) = &template.route.goal {
                    add(goal, &|source| {
                        template_query(&route.id, &template.route.id, source)
                    });
                }
            }
        }
    }
    capabilities
}

impl Journal {
    pub(in crate::task_runtime) fn goal_query_capability(
        &self,
        sequence: u64,
        route: &str,
        source: &str,
    ) -> Result<CapabilityId, HostError> {
        let origin:Option<(String,String)>=self.connection.query_row(
            "SELECT root.route_id,s.template_id FROM runtime_plan_steps s JOIN runtime_jobs root ON root.sequence=s.parent_sequence WHERE s.sequence=?1",
            [sequence as i64],|row|Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(db)?;
        Ok(match origin {
            Some((owner, template)) => template_query(&owner, &template, source),
            None => CapabilityId(format!("runtime.query.{route}.{source}")),
        })
    }
}

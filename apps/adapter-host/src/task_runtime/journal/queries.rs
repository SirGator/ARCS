//! Operator-defined query publishers for static and instantiated goal routes.
use super::{Journal, db};
use crate::HostError;
use crate::task_runtime::{RuntimeConfig, goal::GoalConfig};
use arcs::adapters::{CapabilityContract, CapabilityDescriptor, CapabilityId};
use arcs::core::VersionId;
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn initialize(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS runtime_source_requests(
        input_version TEXT PRIMARY KEY,sequence INTEGER NOT NULL REFERENCES runtime_jobs(sequence),
        round INTEGER NOT NULL CHECK(round>=1));",
        )
        .map_err(db)
}
pub(in crate::task_runtime) struct SourceRequestControl {
    pub input: VersionId,
    pub sequence: u64,
    pub route: String,
    pub abandoned: bool,
    pub paused: bool,
}

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
    pub(in crate::task_runtime) fn bind_source_request(
        &self,
        sequence: u64,
        round: u32,
        input: &VersionId,
    ) -> Result<(), HostError> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO runtime_source_requests VALUES(?1,?2,?3)",
                params![input.0, sequence as i64, round],
            )
            .map_err(db)?;
        let owner: (i64, u32) = self
            .connection
            .query_row(
                "SELECT sequence,round FROM runtime_source_requests WHERE input_version=?1",
                [&input.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if owner != (sequence as i64, round) {
            return Err(super::invalid("source request task identity changed"));
        }
        Ok(())
    }
    /// Examine only live worker inputs, never a task's unbounded history.
    pub(in crate::task_runtime) fn source_request_controls(
        &self,
        inputs: &[VersionId],
    ) -> Result<Vec<SourceRequestControl>, HostError> {
        let mut query = self.connection.prepare(&format!("SELECT j.sequence,j.route_id,
            (j.state NOT IN ('pending','running','awaiting_approval') OR json_extract(j.goal_json,'$.round')<>r.round),
            {} FROM runtime_source_requests r JOIN runtime_jobs j ON j.sequence=r.sequence WHERE r.input_version=?1", super::suspension::PAUSED)).map_err(db)?;
        let mut controls = Vec::new();
        for input in inputs {
            if let Some(control) = query
                .query_row([&input.0], |r| {
                    Ok(SourceRequestControl {
                        input: input.clone(),
                        sequence: r.get::<_, i64>(0)? as u64,
                        route: r.get(1)?,
                        abandoned: r.get(2)?,
                        paused: r.get(3)?,
                    })
                })
                .optional()
                .map_err(db)?
            {
                controls.push(control);
            }
        }
        Ok(controls)
    }

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

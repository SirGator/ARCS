use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef};
use arcs::core::SchemaId;
use arcs::reasoning::{ReasoningMethod, ReasoningResources};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Host, HostError};

fn interval() -> u64 {
    100
}
fn batch() -> usize {
    100
}
fn events() -> usize {
    1000
}
fn capacity() -> usize {
    1000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub config_version: u32,
    pub runtime_id: String,
    pub journal: PathBuf,
    #[serde(default = "interval")]
    pub interval_ms: u64,
    #[serde(default = "events")]
    pub max_events_per_tick: usize,
    #[serde(default = "batch")]
    pub max_jobs_per_tick: usize,
    #[serde(default = "capacity")]
    pub max_pending_jobs: usize,
    pub routes: Vec<ReasoningRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningRoute {
    pub id: String,
    pub source: CapabilityRef,
    pub input_schema_id: SchemaId,
    pub payload_fields: Vec<String>,
    pub reasoner: CapabilityRef,
    pub method: ReasoningMethod,
    pub objective: String,
    pub target_schema_id: SchemaId,
    #[serde(default)]
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub constraints: Value,
    pub budget: ReasoningResources,
    /// Ohne diese Option bleibt die Route eine reine Reasoning-/Review-Route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<super::goal::GoalConfig>,
}

pub(super) fn invalid(message: impl Into<String>) -> HostError {
    HostError::InvalidConfig(format!("task runtime: {}", message.into()))
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
impl RuntimeConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, HostError> {
        let path = path
            .as_ref()
            .canonicalize()
            .map_err(|source| HostError::Io {
                path: path.as_ref().into(),
                source,
            })?;
        let bytes = std::fs::read(&path).map_err(|source| HostError::Io {
            path: path.clone(),
            source,
        })?;
        let mut config: Self =
            serde_json::from_slice(&bytes).map_err(|source| HostError::Json {
                path: path.clone(),
                source,
            })?;
        if config.journal.as_os_str().is_empty() {
            return Err(invalid("empty journal path"));
        }
        config.journal = path.parent().unwrap().join(&config.journal);
        Ok(config)
    }

    pub(super) fn identity(&self, host: &Host) -> Result<Value, HostError> {
        if self.config_version != 1
            || !identifier(&self.runtime_id)
            || !(1..=60_000).contains(&self.interval_ms)
            || !(1..=10_000).contains(&self.max_events_per_tick)
            || !(1..=1000).contains(&self.max_jobs_per_tick)
            || !(1..=32).contains(&self.routes.len())
            || !(self.routes.len()..=100_000).contains(&self.max_pending_jobs)
        {
            return Err(invalid(
                "invalid version, identity, polling limits, capacity or routes",
            ));
        }
        let mut ids = HashSet::new();
        let mut manifests: BTreeMap<String, AdapterManifest> = BTreeMap::new();
        for route in &self.routes {
            let mut fields = HashSet::new();
            if !identifier(&route.id)
                || !ids.insert(&route.id)
                || route.objective.trim().is_empty()
                || route.objective.len() > 4096
                || route.payload_fields.is_empty()
                || route
                    .payload_fields
                    .iter()
                    .any(|field| field.trim().is_empty() || !fields.insert(field))
                || !route.budget.supports(route.method)
            {
                return Err(invalid(format!("invalid route {}", route.id)));
            }
            let (source, descriptor) = host
                .registry()
                .authorized_capability(&route.source.adapter_id, &route.source.capability_id)
                .map_err(HostError::Registration)?;
            if !matches!(&descriptor.contract, CapabilityContract::Input {emits} | CapabilityContract::Observe {emits}
                if emits.as_slice() == std::slice::from_ref(&route.input_schema_id))
            {
                return Err(invalid(
                    "source must Input or Observe exactly the configured schema",
                ));
            }
            manifests.insert(route.source.adapter_id.0.clone(), source.manifest().clone());
            let (reasoner, descriptor) = host
                .registry()
                .authorized_capability(&route.reasoner.adapter_id, &route.reasoner.capability_id)
                .map_err(HostError::Registration)?;
            if !descriptor.contract.is_reasoning()
                || !descriptor
                    .contract
                    .emitted_schemas()
                    .contains(&route.target_schema_id)
            {
                return Err(invalid("reasoner contract does not match target schema"));
            }
            let limits = reasoner
                .grant()
                .reasoning_limits_v2
                .as_ref()
                .ok_or_else(|| invalid("reasoner needs V2 limits"))?;
            if !limits.methods.contains(&route.method)
                || !route.budget.fits_within(&limits.resources)
            {
                return Err(invalid("route method or budget exceeds reasoning grant"));
            }
            let schema = host
                .schemas()
                .get(&route.target_schema_id)
                .ok_or_else(|| invalid("unknown target schema"))?;
            if schema.artifact_type != "candidate" && !schema.artifact_type.ends_with("_candidate")
            {
                return Err(invalid("target schema must describe candidates"));
            }
            manifests.insert(
                route.reasoner.adapter_id.0.clone(),
                reasoner.manifest().clone(),
            );
            let mut allowed = HashSet::new();
            for capability in &route.allowed_capabilities {
                if !allowed.insert(capability) {
                    return Err(invalid("duplicate allowed capability"));
                }
                let (registered, _) = host
                    .registry()
                    .authorized_capability(&capability.adapter_id, &capability.capability_id)
                    .map_err(HostError::Registration)?;
                manifests.insert(
                    capability.adapter_id.0.clone(),
                    registered.manifest().clone(),
                );
            }
            if let Some(goal) = &route.goal {
                goal.validate(host, route, &mut manifests)?;
            }
        }
        // Polling budgets may change. Task semantics and actual manifests may not.
        Ok(json!({"runtime_id":self.runtime_id,"routes":self.routes,"manifests":manifests}))
    }
}

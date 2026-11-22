//! Deterministic graph search over operator-authorized Transform contracts.
use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::TransformError;
use crate::adapters::{AdapterRegistry, CapabilityContract, CapabilityRef, ProducerClass};
use crate::core::{SchemaId, SchemaRegistry};
use crate::io::control_types::is_control_type;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformRouting {
    pub output_schema_id: SchemaId,
    pub allowed_capabilities: Vec<CapabilityRef>,
    pub max_steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformHop {
    pub capability: CapabilityRef,
    pub output_schema_id: SchemaId,
}

impl TransformRouting {
    /// Resolve the shortest schema path without contacting any adapter.
    pub fn resolve(
        &self,
        registry: &AdapterRegistry,
        schemas: &SchemaRegistry,
        input_schema_id: &SchemaId,
    ) -> Result<Option<Vec<TransformHop>>, TransformError> {
        if !(1..=32).contains(&self.max_steps) || self.allowed_capabilities.len() > 64 {
            return Err(TransformError::InvalidRoutingPolicy(
                "expected 1..32 steps and at most 64 capabilities",
            ));
        }
        let mut allowed = self.allowed_capabilities.iter().collect::<Vec<_>>();
        allowed.sort_unstable();
        if allowed.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(TransformError::InvalidRoutingPolicy(
                "duplicate allowed capability",
            ));
        }
        if schemas.get(input_schema_id).is_none() {
            return Err(TransformError::MissingSchema(input_schema_id.clone()));
        }
        let target = schemas
            .get(&self.output_schema_id)
            .ok_or_else(|| TransformError::MissingSchema(self.output_schema_id.clone()))?;
        if is_control_type(&target.artifact_type) {
            return Err(TransformError::ControlOutputForbidden(
                self.output_schema_id.clone(),
            ));
        }
        let mut edges = HashMap::<&SchemaId, Vec<TransformHop>>::new();
        for capability in allowed {
            let (adapter, descriptor) = registry
                .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
            let CapabilityContract::Transform { accepts, emits } = &descriptor.contract else {
                return Err(TransformError::CapabilityIsNotTransform(capability.clone()));
            };
            if adapter.grant().producer_class == ProducerClass::Model {
                return Err(TransformError::ModelMustUseReasoning);
            }
            let mut outputs = emits.iter().collect::<Vec<_>>();
            outputs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            for output in outputs {
                let definition = schemas
                    .get(output)
                    .ok_or_else(|| TransformError::MissingSchema(output.clone()))?;
                if is_control_type(&definition.artifact_type) {
                    continue;
                }
                for input in accepts {
                    edges.entry(input).or_default().push(TransformHop {
                        capability: capability.clone(),
                        output_schema_id: output.clone(),
                    });
                }
            }
        }
        let mut pending = VecDeque::from([(input_schema_id.clone(), Vec::new())]);
        let mut visited = HashSet::from([input_schema_id.clone()]);
        while let Some((schema, path)) = pending.pop_front() {
            if schema == self.output_schema_id {
                return Ok(Some(path));
            }
            if path.len() == self.max_steps {
                continue;
            }
            for step in edges.get(&schema).into_iter().flatten() {
                if visited.insert(step.output_schema_id.clone()) {
                    let mut next = path.clone();
                    next.push(step.clone());
                    pending.push_back((step.output_schema_id.clone(), next));
                }
            }
        }
        Ok(None)
    }
}

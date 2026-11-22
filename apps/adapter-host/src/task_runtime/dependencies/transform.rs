use crate::{Host, HostError};
use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef, ProducerClass};
use arcs::core::SchemaId;
use arcs::transform::{TransformInputProjection, validate_output_schema};
use serde::{Deserialize, Serialize};

use super::invalid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyTransform {
    pub source_schema_id: SchemaId,
    pub projection: TransformInputProjection,
    pub capability: CapabilityRef,
    pub output_schema_id: SchemaId,
    pub output_fields: Vec<String>,
}

impl DependencyTransform {
    pub(in crate::task_runtime) fn validate(
        &self,
        host: &Host,
    ) -> Result<AdapterManifest, HostError> {
        self.projection
            .validate(host.schemas())
            .and_then(|_| {
                validate_output_schema(host.schemas(), &self.output_schema_id).map(|_| ())
            })
            .map_err(|error| invalid(format!("invalid dependency transform schemas: {error:?}")))?;
        let (adapter, capability) = host
            .registry()
            .authorized_capability(&self.capability.adapter_id, &self.capability.capability_id)
            .map_err(HostError::Registration)?;
        if host.schemas().get(&self.source_schema_id).is_none()
            || adapter.grant().producer_class == ProducerClass::Model
            || !matches!(&capability.contract, CapabilityContract::Transform { accepts, emits }
                if accepts.contains(&self.projection.schema_id) && emits.contains(&self.output_schema_id))
        {
            return Err(invalid(
                "dependency transform does not match registered schemas, role or contract",
            ));
        }
        Ok(adapter.manifest().clone())
    }

    pub(in crate::task_runtime) fn matches_fields(&self, fields: &[String]) -> bool {
        self.projection.payload_fields.len() == fields.len()
            && self
                .projection
                .payload_fields
                .iter()
                .all(|field| fields.contains(field))
    }
}

impl super::TaskRuntime {
    pub(super) fn transform_dependencies(
        &self,
        host: &mut Host,
        sequence: u64,
        route: &super::ReasoningRoute,
        bindings: &mut [super::DependencyBinding],
    ) -> Result<bool, HostError> {
        let mut ready = true;
        for binding in bindings {
            let Some(transform) = route.dependency_transforms.get(&binding.route_id) else {
                continue;
            };
            let mut converted = Vec::with_capacity(binding.context.len());
            for selection in &binding.context {
                if !transform.matches_fields(&selection.payload_fields) {
                    return Err(invalid(
                        "bound export fields differ from the transform projection",
                    ));
                }
                let source = host
                    .store()
                    .get(&selection.version_id)
                    .map_err(HostError::Store)?
                    .ok_or_else(|| invalid("dependency transform source artifact is missing"))?;
                if source.schema_id != transform.source_schema_id {
                    return Err(invalid("dependency transform source schema changed"));
                }
                let projection = transform
                    .projection
                    .materialize(
                        &source.version_id,
                        host.store(),
                        host.schemas(),
                        &arcs::core::SystemClock,
                    )
                    .map_err(|source| HostError::Transform {
                        adapter: transform.capability.adapter_id.clone(),
                        source,
                    })?;
                let result = host.poll_dependency_transform(
                    (&self.journal.path, sequence),
                    &transform.capability,
                    &projection.version_id,
                    &transform.output_schema_id,
                    self.config.max_concurrent_transforms,
                )?;
                let Some(result) = result else {
                    ready = false;
                    continue;
                };
                if transform
                    .output_fields
                    .iter()
                    .any(|field| result.payload.get(field).is_none())
                {
                    return Err(invalid("a configured transform output field is missing"));
                }
                converted.push(arcs::reasoning::ContextSelection {
                    version_id: result.version_id,
                    payload_fields: transform.output_fields.clone(),
                });
            }
            binding.context = converted;
        }
        Ok(ready)
    }
}

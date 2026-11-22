//! Kontrollierte, deterministische Ableitungen aus persistierten Artefakten.

use crate::adapters::{AdapterCallError, AdapterManifest};
pub use arcs_adapter_protocol::{TransformInvocation, TransformResponse};

mod error;
mod projection;
mod routing;
mod service;
pub use error::TransformError;
pub use projection::{PROJECTION_PUBLISHER, TransformInputProjection};
pub use routing::{TransformHop, TransformRouting};
pub use service::{
    PreparedTransform, TransformPreparation, TransformService, transform_invocation_id,
};

/// Resolve a registered data schema permitted as a Transform result.
pub fn validate_output_schema<'a>(
    schemas: &'a crate::core::SchemaRegistry,
    schema_id: &crate::core::SchemaId,
) -> Result<&'a crate::core::SchemaDefinition, TransformError> {
    let definition = schemas
        .get(schema_id)
        .ok_or_else(|| TransformError::MissingSchema(schema_id.clone()))?;
    if super::control_types::is_control_type(&definition.artifact_type) {
        return Err(TransformError::ControlOutputForbidden(schema_id.clone()));
    }
    Ok(definition)
}

/// Ein Transform verarbeitet Daten ohne externe Effekte. Identische Aufrufe
/// müssen dasselbe Ergebnis liefern, auch nach verlorenen Antworten.
pub trait TransformAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;
    fn transform(
        &self,
        invocation: &TransformInvocation,
    ) -> Result<TransformResponse, AdapterCallError>;
}

#[cfg(test)]
mod tests;

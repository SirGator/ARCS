//! Derive eligibility from the same data descriptors used at dispatch.
use super::*;
use crate::adapters::input_data::{InputDataDescriptor, InputDataSelection, InputDataService};
use crate::reasoning::{ContextSelection, ReasoningContextItem};

pub fn describe_inputs(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    context: &[ContextSelection],
    selections: &[InputDataSelection],
) -> Result<Vec<InputDataDescriptor>, ReasoningError> {
    let mut seen = HashSet::new();
    let versions = selections
        .iter()
        .map(|input| &input.source_version_id)
        .filter(|version| seen.insert(*version));
    let mut selected = Vec::new();
    for version in versions {
        let choice = context
            .iter()
            .find(|choice| &choice.version_id == version)
            .ok_or_else(|| invalid("binary input is outside the selected context"))?;
        let artifact = store
            .get(version)?
            .ok_or_else(|| ReasoningError::MissingContextArtifact(version.clone()))?;
        let payload =
            super::super::service::select_payload_fields(&artifact, &choice.payload_fields)?;
        selected.push(ReasoningContextItem {
            version_id: artifact.version_id,
            artifact_type: artifact.artifact_type,
            schema_id: artifact.schema_id,
            trust_level: artifact.trust.level,
            payload,
        });
    }
    Ok(InputDataService::new(registry, schemas, store).describe(&selected, selections)?)
}

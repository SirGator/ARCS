//! Beispiel eines reinen, deterministischen Transforms ohne Core-Abhängigkeit.

use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId, SchemaId,
    TransformInvocation, TransformResponse,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use serde::{Deserialize, Serialize};

pub const ADAPTER: &str = "example.normalize";
pub const CAPABILITY: &str = "number.normalize";
pub const INPUT_SCHEMA: &str = "arcs.number.raw.v1";
pub const OUTPUT_SCHEMA: &str = "arcs.number.normalized.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberInput {
    pub value: f64,
    pub scale: f64,
    pub offset: f64,
    pub unit: String,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NumberOutput {
    pub value: f64,
    pub unit: String,
}

/// Einheiten werden ausdrücklich vorgegeben, nicht aus Zahlen erschlossen.
pub fn normalize(input: NumberInput) -> Result<NumberOutput, AdapterError> {
    if !input.value.is_finite()
        || !input.scale.is_finite()
        || !input.offset.is_finite()
        || input.unit.trim().is_empty()
        || input.unit.len() > 64
        || input.unit.chars().any(char::is_control)
    {
        return Err(AdapterError::rejected("invalid normalization input"));
    }
    let value = input.value * input.scale + input.offset;
    if !value.is_finite() {
        return Err(AdapterError::rejected("normalization overflow"));
    }
    Ok(NumberOutput {
        value: if value == 0.0 { 0.0 } else { value },
        unit: input.unit,
    })
}

pub struct Normalizer;

impl Adapter for Normalizer {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId(ADAPTER.into()),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId(CAPABILITY.into()),
                contract: CapabilityContract::Transform {
                    accepts: vec![SchemaId(INPUT_SCHEMA.into())],
                    emits: vec![SchemaId(OUTPUT_SCHEMA.into())],
                },
                required_permissions: vec!["data.transform".into()],
            }],
        }
    }

    fn transform(
        &mut self,
        invocation: TransformInvocation,
    ) -> Result<TransformResponse, AdapterError> {
        if invocation.capability.adapter_id.0 != ADAPTER
            || invocation.capability.capability_id.0 != CAPABILITY
            || invocation.input_schema_id.0 != INPUT_SCHEMA
            || invocation.output_schema_id.0 != OUTPUT_SCHEMA
        {
            return Err(AdapterError::rejected("normalization contract mismatch"));
        }
        let input: NumberInput = serde_json::from_value(invocation.input_payload)
            .map_err(|error| AdapterError::rejected(error.to_string()))?;
        let payload = serde_json::to_value(normalize(input)?)
            .map_err(|error| AdapterError::rejected(error.to_string()))?;
        Ok(TransformResponse {
            external_reference: format!("normalized:{}", invocation.invocation_id),
            invocation_id: invocation.invocation_id,
            payload,
        })
    }
}

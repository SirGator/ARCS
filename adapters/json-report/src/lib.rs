//! Deterministische Darstellung beliebiger deklarierter JSON-Verträge.

use arcs_adapter_protocol::{
    ADAPTER_PROTOCOL_VERSION, AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor,
    CapabilityId, SchemaId, TransformInvocation, TransformResponse,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use serde::Deserialize;
use serde_json::json;

pub const CAPABILITY: &str = "json.report";
pub const COMPACT_CAPABILITY: &str = "report.compact";
pub const OUTPUT_SCHEMA: &str = "arcs.report.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub adapter_id: AdapterId,
    pub accepts: Vec<SchemaId>,
}

pub struct JsonReport {
    manifest: AdapterManifest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    title: String,
    body: String,
}

impl JsonReport {
    pub fn new(profile: Profile) -> Result<Self, String> {
        let manifest = AdapterManifest {
            protocol_version: ADAPTER_PROTOCOL_VERSION,
            adapter_id: profile.adapter_id,
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![
                CapabilityDescriptor {
                    id: CapabilityId(CAPABILITY.into()),
                    contract: CapabilityContract::Transform {
                        accepts: profile.accepts,
                        emits: vec![SchemaId(OUTPUT_SCHEMA.into())],
                    },
                    required_permissions: vec!["data.transform".into()],
                },
                CapabilityDescriptor {
                    id: CapabilityId(COMPACT_CAPABILITY.into()),
                    contract: CapabilityContract::Transform {
                        accepts: vec![SchemaId(OUTPUT_SCHEMA.into())],
                        emits: vec![SchemaId(OUTPUT_SCHEMA.into())],
                    },
                    required_permissions: vec!["data.transform".into()],
                },
            ],
        };
        manifest.validate().map_err(|error| format!("{error:?}"))?;
        Ok(Self { manifest })
    }
}

impl Adapter for JsonReport {
    fn manifest(&self) -> AdapterManifest {
        self.manifest.clone()
    }

    fn transform(
        &mut self,
        invocation: TransformInvocation,
    ) -> Result<TransformResponse, AdapterError> {
        let descriptor = self
            .manifest
            .capability(&invocation.capability.capability_id)
            .ok_or_else(|| AdapterError::rejected("unknown JSON report capability"))?;
        let contract = &descriptor.contract;
        if invocation.capability.adapter_id != self.manifest.adapter_id
            || !contract
                .accepted_schemas()
                .contains(&invocation.input_schema_id)
            || invocation.output_schema_id.0 != OUTPUT_SCHEMA
            || invocation.invocation_id.trim().is_empty()
            || invocation.input_version_id.0.trim().is_empty()
        {
            return Err(AdapterError::rejected("JSON report contract mismatch"));
        }
        // Nur Darstellung: keine fachliche Interpretation, keine Behauptung,
        // dass der Eingang eine wahre oder bereits freigegebene Aussage ist.
        let (title, body) = if invocation.capability.capability_id.0 == COMPACT_CAPABILITY {
            let report: Report = serde_json::from_value(invocation.input_payload)
                .map_err(|error| AdapterError::rejected(error.to_string()))?;
            if report.title.trim().is_empty()
                || report.title.len() > 256
                || report.title.chars().any(char::is_control)
                || report.body.len() > 60 * 1024
            {
                return Err(AdapterError::rejected("invalid report"));
            }
            let value: serde_json::Value = serde_json::from_str(&report.body)
                .map_err(|error| AdapterError::rejected(error.to_string()))?;
            (
                report.title,
                serde_json::to_string(&value)
                    .map_err(|error| AdapterError::rejected(error.to_string()))?,
            )
        } else {
            let body = serde_json::to_string_pretty(&json!({
                "schema_id": invocation.input_schema_id,
                "version_id": invocation.input_version_id,
                "payload": invocation.input_payload,
            }))
            .map_err(|error| AdapterError::rejected(error.to_string()))?;
            ("JSON artifact".into(), body)
        };
        let payload = json!({"title":title, "body":body});
        if body.len() > 60 * 1024
            || serde_json::to_vec(&payload)
                .map_err(|error| AdapterError::rejected(error.to_string()))?
                .len()
                > invocation.max_output_bytes
        {
            return Err(AdapterError::rejected("JSON report exceeds output limit"));
        }
        Ok(TransformResponse {
            external_reference: format!("report:{}", invocation.invocation_id),
            invocation_id: invocation.invocation_id,
            payload,
        })
    }
}

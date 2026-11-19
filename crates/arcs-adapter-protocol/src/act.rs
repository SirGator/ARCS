use crate::{CapabilityRef, SchemaId, VersionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Vom Core erzeugter Auftrag aus einem materialisierten Action-Artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActInvocation {
    pub invocation_id: String,
    pub capability: CapabilityRef,
    pub action_version_id: VersionId,
    pub action_schema_id: SchemaId,
    pub payload: Value,
    pub result_schema_id: SchemaId,
}

/// Core-issued reservation revision for a configured Act controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActFence {
    pub authority: String,
    pub resource: String,
    pub revision: u64,
    pub reservation_version_id: VersionId,
}

impl ActFence {
    pub fn valid_resource(resource: &str) -> bool {
        (1..=64).contains(&resource.len())
            && resource
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    }

    pub fn is_valid(&self) -> bool {
        self.authority.len() == 32
            && self
                .authority
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && Self::valid_resource(&self.resource)
            && (1..=i64::MAX as u64).contains(&self.revision)
            && (1..=256).contains(&self.reservation_version_id.0.len())
            && !self
                .reservation_version_id
                .0
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
    }
}

/// Korrelierte, weiterhin untrusted Rückgabe eines Act-Adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub result_payload: Value,
}

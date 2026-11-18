//! Bounded advisory claims supplied by an adapter, never an authorization.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescription {
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
}

impl CapabilityDescription {
    /// Descriptions only assist discovery. They do not establish that a
    /// precondition is satisfied, an effect occurred or a resource is free.
    pub fn is_valid(&self) -> bool {
        text(&self.purpose, 1024)
            && self.tags.len() <= 32
            && self.tags.iter().all(|tag| {
                text(tag, 64)
                    && tag
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
            && self.tags.iter().collect::<HashSet<_>>().len() == self.tags.len()
            && [
                &self.preconditions,
                &self.effects,
                &self.constraints,
                &self.resources,
            ]
            .into_iter()
            .all(|items| {
                items.len() <= 16
                    && items.iter().all(|item| text(item, 256))
                    && items.iter().collect::<HashSet<_>>().len() == items.len()
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 8192)
    }
}

fn text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
        ManifestError, SchemaId,
    };
    use serde_json::json;

    fn manifest(description: Option<CapabilityDescription>) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("test".into()),
            adapter_version: "1".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("read".into()),
                contract: CapabilityContract::Observe {
                    emits: vec![SchemaId("data".into())],
                },
                required_permissions: vec!["read".into()],
                description,
            }],
        }
    }

    #[test]
    fn old_manifests_round_trip_without_optional_fields_and_claims_are_bounded() {
        let old = json!({"protocol_version":1,"adapter_id":"test","adapter_version":"1","capabilities":[{"id":"read","contract":{"kind":"observe","emits":["data"]},"required_permissions":["read"]}]});
        let parsed: AdapterManifest = serde_json::from_value(old.clone()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), old);
        let description:CapabilityDescription=serde_json::from_value(json!({"purpose":"Read temperature","tags":["sensor"],"preconditions":["Device connected"],"effects":["Returns a measurement"],"constraints":["No calibration guarantee"],"resources":["Sensor connection"]})).unwrap();
        manifest(Some(description.clone())).validate().unwrap();
        for case in ["empty", "duplicate", "count", "bytes", "control", "total"] {
            let mut invalid = description.clone();
            match case {
                "empty" => invalid.purpose = " ".into(),
                "duplicate" => invalid.tags = vec!["sensor".into(); 2],
                "count" => invalid.effects = (0..17).map(|n| n.to_string()).collect(),
                "bytes" => invalid.purpose = "a".repeat(1025),
                "control" => invalid.constraints = vec!["unsafe\nline".into()],
                _ => {
                    invalid.preconditions =
                        (0..16).map(|n| format!("{n}{}", "a".repeat(250))).collect();
                    invalid.effects = invalid.preconditions.clone();
                    invalid.resources = invalid.preconditions.clone();
                }
            }
            assert!(
                matches!(
                    manifest(Some(invalid)).validate(),
                    Err(ManifestError::InvalidCapabilityDescription(_))
                ),
                "{case}"
            );
        }
    }

    #[test]
    fn claims_cannot_supply_verification_or_switch_off_action_requirements() {
        assert!(
            serde_json::from_value::<CapabilityDescription>(
                json!({"purpose":"Harmless action","verified":true})
            )
            .is_err()
        );
        let mut described = manifest(Some(
            serde_json::from_value(
                json!({"purpose":"Safe with no approval","effects":["No external effect"]}),
            )
            .unwrap(),
        ));
        described.capabilities[0].contract = CapabilityContract::Act {
            accepts: vec![SchemaId("candidate".into())],
            emits: vec![SchemaId("data".into())],
            idempotent: false,
        };
        assert!(matches!(
            described.validate(),
            Err(ManifestError::UnsafeExternalAction(_))
        ));
        described.capabilities[0].contract = CapabilityContract::Act {
            accepts: vec![SchemaId("candidate".into())],
            emits: vec![SchemaId("data".into())],
            idempotent: true,
        };
        described.capabilities[0].required_permissions.clear();
        assert!(matches!(
            described.validate(),
            Err(ManifestError::UnsafeExternalAction(_))
        ));
    }
}

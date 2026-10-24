//! Fachliche Demo-Verträge und Profil des eigenständigen Lüfter-Adapters.

use arcs_adapter_protocol::{
    ADAPTER_PROTOCOL_VERSION, AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor,
    CapabilityId,
};
use arcs_adapter_protocol::{SchemaId, VersionId};
use serde::{Deserialize, Serialize};

pub const ADAPTER: &str = "demo.fan";
pub const TEMPERATURE_REQUEST: &str = "arcs.request.temperature.v1";
pub const FAN_REQUEST: &str = "arcs.request.fan.v1";
pub const TEMPERATURE: &str = "arcs.measurement.temperature.v1";
pub const FAN_STATE: &str = "arcs.state.fan.v1";
pub const CANDIDATE: &str = "arcs.fan_candidate.v1";
pub const RESULT: &str = "arcs.execution_result.fan.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub temperature_device: String,
    pub fan_device: String,
    pub raw_temperature: f64,
    pub scale: f64,
    pub offset: f64,
    pub unit: String,
    pub activate_above_celsius: f64,
    pub max_auto_approve_celsius: f64,
}

impl Profile {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let profile: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if profile.temperature_device.trim().is_empty()
            || profile.fan_device.trim().is_empty()
            || profile.temperature_device.len() > 128
            || profile.fan_device.len() > 128
            || profile.unit != "Cel"
            || !profile.temperature().is_finite()
            || !profile.activate_above_celsius.is_finite()
            || !profile.max_auto_approve_celsius.is_finite()
            || profile.max_auto_approve_celsius <= profile.activate_above_celsius
        {
            return Err("invalid simulated fan profile".into());
        }
        Ok(profile)
    }

    pub fn temperature(&self) -> f64 {
        self.raw_temperature * self.scale + self.offset
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanState {
    pub device_id: String,
    pub enabled: bool,
    pub actuation_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanCommand {
    pub device_id: String,
    pub enabled: bool,
}

/// Schema-Dokumente zum Registrieren durch einen Host, ohne Core-Abhängigkeit.
pub const SCHEMAS: &[&str] = &[
    include_str!("../schemas/temperature.schema.json"),
    include_str!("../schemas/request-temperature.schema.json"),
    include_str!("../schemas/request-fan.schema.json"),
    include_str!("../schemas/fan-candidate.schema.json"),
    include_str!("../schemas/fan-state.schema.json"),
    include_str!("../schemas/fan-execution-result.schema.json"),
    include_str!("../schemas/fan-cycle.schema.json"),
];

pub fn manifest() -> AdapterManifest {
    manifest_with_id(AdapterId(ADAPTER.into()))
}

/// Eine Installation darf dieselben lokalen Fähigkeiten wie andere
/// Installationen anbieten; ihre Adapter-ID unterscheidet die Ziele.
pub fn manifest_with_id(adapter_id: AdapterId) -> AdapterManifest {
    let descriptor = |id: &str, contract, permission: &str| CapabilityDescriptor {
        id: CapabilityId(id.into()),
        contract,
        required_permissions: vec![permission.into()],
    };
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id,
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            descriptor(
                "temperature.read",
                CapabilityContract::Request {
                    accepts: vec![SchemaId(TEMPERATURE_REQUEST.into())],
                    emits: vec![SchemaId(TEMPERATURE.into())],
                },
                "sensor.read",
            ),
            descriptor(
                "fan.read",
                CapabilityContract::Request {
                    accepts: vec![SchemaId(FAN_REQUEST.into())],
                    emits: vec![SchemaId(FAN_STATE.into())],
                },
                "sensor.read",
            ),
            descriptor(
                "temperature.observe",
                CapabilityContract::Observe {
                    emits: vec![SchemaId(TEMPERATURE.into())],
                },
                "sensor.read",
            ),
            descriptor(
                "fan.observe",
                CapabilityContract::Observe {
                    emits: vec![SchemaId(FAN_STATE.into())],
                },
                "sensor.read",
            ),
            descriptor(
                "fan.set",
                CapabilityContract::Act {
                    accepts: vec![SchemaId(CANDIDATE.into())],
                    emits: vec![SchemaId(RESULT.into())],
                    idempotent: true,
                },
                "fan.write",
            ),
        ],
    }
}

/// Gezielte Geräteabfrage; unbekannte Felder werden abgewiesen.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceQuery {
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResult {
    pub action_version: VersionId,
    pub result: FanState,
}

/// Mitgeliefertes Beispielprofil.
pub const DEFAULT_PROFILE: &str = include_str!("../config/profile.json");

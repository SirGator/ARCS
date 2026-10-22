//! Eigenständiger Beispieladapter; Gerätewissen bleibt außerhalb des Cores.

use std::collections::HashSet;
use std::io::{self, BufReader};

use arcs_adapter_protocol::ObservationMessage;
use arcs_adapter_protocol::SchemaId;
use arcs_adapter_protocol::{
    ADAPTER_PROTOCOL_VERSION, AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor,
    CapabilityId,
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingObservation, serve};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    device_id: String,
    capability_id: CapabilityId,
    schema_id: SchemaId,
    field: String,
    scale: f64,
    offset: f64,
    unit: String,
    samples: Vec<Sample>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    id: String,
    raw: Value,
}

fn run() -> Result<(), String> {
    let bytes = match std::env::args_os().nth(1) {
        Some(path) => std::fs::read(path).map_err(|e| e.to_string())?,
        None => include_bytes!("../config/profile.json").to_vec(),
    };
    let profile: Profile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if profile.device_id.trim().is_empty()
        || profile.unit.trim().is_empty()
        || !profile.scale.is_finite()
        || !profile.offset.is_finite()
    {
        return Err("invalid device profile".into());
    }
    // Ein Profil beschreibt explizit bekannte Semantik. Es erkennt keine
    // unbekannten Geräte und bestätigt keine automatisch erschlossene Einheit.
    let mut ids = HashSet::new();
    for sample in &profile.samples {
        if sample.id.trim().is_empty() || sample.id.len() > 128 || !ids.insert(&sample.id) {
            return Err("invalid or duplicate sample ID".into());
        }
        let raw = sample
            .raw
            .get(&profile.field)
            .and_then(Value::as_f64)
            .ok_or("profile field must contain a number")?;
        if !(raw * profile.scale + profile.offset).is_finite() {
            return Err("non-finite normalized measurement".into());
        }
    }
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: profile.adapter_id.clone(),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: profile.capability_id.clone(),
            contract: CapabilityContract::Observe {
                emits: vec![profile.schema_id.clone()],
            },
            required_permissions: vec!["sensor.read".into()],
        }],
    };
    let mut sensor = Sensor {
        profile,
        manifest,
        next: 0,
        last_ack: None,
    };
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    serve(&mut sensor, &mut input, &mut output).map_err(|error| error.to_string())
}

struct Sensor {
    profile: Profile,
    manifest: AdapterManifest,
    next: usize,
    last_ack: Option<String>,
}

impl Adapter for Sensor {
    fn manifest(&self) -> AdapterManifest {
        self.manifest.clone()
    }

    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        Ok(self.profile.samples.get(self.next).map(|sample| {
            let raw = sample.raw[&self.profile.field].as_f64().expect("validated sample");
            PendingObservation {
                message_id: sample.id.clone(),
                observation: ObservationMessage {
                    capability_id: self.profile.capability_id.clone(),
                    external_subject: Some(self.profile.device_id.clone()),
                    external_reference: format!("simulated:{}", sample.id),
                    payload: json!({"value": raw * self.profile.scale + self.profile.offset, "unit": self.profile.unit}),
                },
            }
        }))
    }

    fn acknowledge(&mut self, message_id: &str) -> Result<(), AdapterError> {
        if self.last_ack.as_deref() == Some(message_id) {
            return Ok(());
        }
        if !self
            .profile
            .samples
            .get(self.next)
            .is_some_and(|sample| sample.id == message_id)
        {
            return Err(AdapterError::rejected("unknown acknowledgement"));
        }
        self.next += 1;
        self.last_ack = Some(message_id.into());
        Ok(())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("simulated sensor: {error}");
        std::process::exit(1);
    }
}

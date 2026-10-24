//! Geräteunabhängiger Kontext einer unveränderlichen Sensoraufnahme.
pub const CAPTURE_CONTEXT_SCHEMA: &str = "arcs.data.capture_context.v1";
pub const CAPTURE_CONTEXT_SCHEMA_JSON: &str =
    include_str!("../schemas/capture-context.schema.json");
use crate::CapabilityRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTime {
    pub clock_id: String,
    /// Zeitpunkt der Aufnahme, unabhängig vom späteren Core-Eingang.
    pub unix_time_ms: u64,
    /// None bedeutet unbekannt, nicht fehlerfrei oder synchronisiert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateStamp {
    pub stream_id: String,
    pub revision: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleStatus {
    Valid,
    Invalid,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleOrigin {
    Measured,
    Simulated,
    Derived,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleUncertainty {
    Unknown,
    IdealizedSimulation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleQuality {
    pub status: SampleStatus,
    pub origin: SampleOrigin,
    pub uncertainty: SampleUncertainty,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureContext {
    pub capture_id: String,
    pub source: CapabilityRef,
    pub sensor_id: String,
    pub captured_at: CaptureTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StateStamp>,
    pub coordinate_frame: String,
    pub quality: SampleQuality,
}
impl CaptureContext {
    pub fn is_valid(&self) -> bool {
        let text = |value: &str| {
            !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        };
        [
            &self.capture_id,
            &self.source.adapter_id.0,
            &self.source.capability_id.0,
            &self.sensor_id,
            &self.captured_at.clock_id,
            &self.coordinate_frame,
        ]
        .iter()
        .all(|value| text(value))
            && self.captured_at.unix_time_ms <= i64::MAX as u64
            && self
                .state
                .as_ref()
                .is_none_or(|state| text(&state.stream_id) && state.revision <= i64::MAX as u64)
            && (self.quality.uncertainty != SampleUncertainty::IdealizedSimulation
                || self.quality.origin == SampleOrigin::Simulated)
    }
    /// Nominelles Alter nur innerhalb derselben Uhrdomäne. Keine Aussage über
    /// Uhrsynchronisation oder die unbekannte Messunsicherheit.
    pub fn nominal_age_ms(&self, now_unix_ms: u64, clock_id: &str) -> Option<u64> {
        (self.is_valid() && self.captured_at.clock_id == clock_id)
            .then(|| now_unix_ms.checked_sub(self.captured_at.unix_time_ms))
            .flatten()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> CaptureContext {
        CaptureContext {
            capture_id: "frame-1".into(),
            source: CapabilityRef::new("sensor", "capture"),
            sensor_id: "front".into(),
            captured_at: CaptureTime {
                clock_id: "sensor.utc".into(),
                unix_time_ms: 1000,
                uncertainty_ms: None,
            },
            state: Some(StateStamp {
                stream_id: "state-1".into(),
                revision: 4,
            }),
            coordinate_frame: "world".into(),
            quality: SampleQuality {
                status: SampleStatus::Valid,
                origin: SampleOrigin::Simulated,
                uncertainty: SampleUncertainty::IdealizedSimulation,
            },
        }
    }
    #[test]
    fn age_requires_matching_clock_and_does_not_invent_precision() {
        let capture = sample();
        assert!(capture.is_valid());
        assert_eq!(capture.nominal_age_ms(1100, "sensor.utc"), Some(100));
        assert_eq!(capture.nominal_age_ms(1100, "other.utc"), None);
        assert_eq!(capture.nominal_age_ms(900, "sensor.utc"), None);
        assert_eq!(capture.captured_at.uncertainty_ms, None);
        let mut unknown_state = capture.clone();
        unknown_state.state = None;
        unknown_state.quality.uncertainty = SampleUncertainty::Unknown;
        assert!(unknown_state.is_valid());
        assert!(
            serde_json::to_value(unknown_state)
                .unwrap()
                .get("state")
                .is_none()
        );
        let mut invalid = capture;
        invalid.quality.origin = SampleOrigin::Measured;
        assert!(!invalid.is_valid());
    }
}

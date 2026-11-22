//! Zeit- und Ereignisaktivierung derselben Runtime; keine eigenen Worker.
mod recurrence;
pub(super) use recurrence::occurrence_route_id;
pub use recurrence::{CheckOccurrence, CheckSeries, Recurrence};
mod series_control;
pub use series_control::{
    SeriesChange, SeriesControlCommand, SeriesControlEvent, SeriesControlReceipt, SeriesState,
};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef};
use arcs::core::{Artifact, SchemaId, SubjectId, VersionId};
use serde::{Deserialize, Serialize};

use super::{TaskRuntime, config::invalid};
use crate::{Host, HostError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Activation {
    AtTime {
        unix_ms: u64,
    },
    AfterDelay {
        delay_ms: u64,
    },
    OnEvent {
        source: CapabilityRef,
        schema_id: SchemaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<SubjectId>,
    },
    OnEventOrAfterDelay {
        source: CapabilityRef,
        schema_id: SchemaId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<SubjectId>,
        delay_ms: u64,
    },
}

impl Activation {
    pub(super) fn validate(
        &self,
        host: &Host,
        manifests: &mut BTreeMap<String, AdapterManifest>,
    ) -> Result<(), HostError> {
        match self {
            Self::AtTime { unix_ms } if *unix_ms > i64::MAX as u64 => {
                return Err(invalid("activation timestamp exceeds SQLite range"));
            }
            Self::AfterDelay { delay_ms } | Self::OnEventOrAfterDelay { delay_ms, .. }
                if *delay_ms > 315_576_000_000 =>
            {
                return Err(invalid("activation delay must be at most ten years"));
            }
            _ => {}
        }
        if let Some((source, schema_id, subject)) = self.event() {
            if subject.is_some_and(|s| s.0.is_empty() || s.0.len() > 256) {
                return Err(invalid("activation subject must contain 1..256 bytes"));
            }
            let (adapter, capability) = host
                .registry()
                .authorized_capability(&source.adapter_id, &source.capability_id)
                .map_err(HostError::Registration)?;
            if !matches!(&capability.contract,
                CapabilityContract::Input { emits } | CapabilityContract::Observe { emits }
                if emits.contains(schema_id))
            {
                return Err(invalid(
                    "activation event needs an authorized Input or Observe schema",
                ));
            }
            manifests.insert(source.adapter_id.0.clone(), adapter.manifest().clone());
        }
        Ok(())
    }

    pub(super) fn due_at(&self, now: u64) -> Result<Option<u64>, HostError> {
        match self {
            Self::AtTime { unix_ms } => Ok(Some(*unix_ms)),
            Self::AfterDelay { delay_ms } | Self::OnEventOrAfterDelay { delay_ms, .. } => now
                .checked_add(*delay_ms)
                .filter(|time| *time <= i64::MAX as u64)
                .map(Some)
                .ok_or_else(|| invalid("activation deadline overflow")),
            Self::OnEvent { .. } => Ok(None),
        }
    }

    fn event(&self) -> Option<(&CapabilityRef, &SchemaId, Option<&SubjectId>)> {
        match self {
            Self::OnEvent {
                source,
                schema_id,
                subject,
            }
            | Self::OnEventOrAfterDelay {
                source,
                schema_id,
                subject,
                ..
            } => Some((source, schema_id, subject.as_ref())),
            Self::AtTime { .. } | Self::AfterDelay { .. } => None,
        }
    }

    pub(super) fn event_source(&self) -> Option<&CapabilityRef> {
        self.event().map(|(source, _, _)| source)
    }

    pub(super) fn matches(&self, artifact: &Artifact) -> bool {
        self.event().is_some_and(|(source, schema_id, subject)| {
            super::matches_capability(source, schema_id, artifact)
                && subject.is_none_or(|expected| artifact.subject.as_ref() == Some(expected))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleState {
    WaitingTime,
    WaitingEvent,
    Ready,
}

/// Operativer Aktivierungsbeleg, gebunden an den ursprünglichen Core-Input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub sequence: u64,
    /// Aktivierungsrevision derselben Aufgabe; bestehende Belege beginnen bei 0.
    #[serde(default)]
    pub revision: u32,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub activation: Activation,
    pub state: ScheduleState,
    pub accepted_at_unix_ms: u64,
    pub not_before_unix_ms: Option<u64>,
    /// Fälligkeit des Zeitzweigs, falls vorher kein Signal freigibt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_at_unix_ms: Option<u64>,
    pub event_cursor: u64,
    pub released_at_unix_ms: Option<u64>,
    pub wake_event_version_id: Option<VersionId>,
}

pub(super) fn now_unix_ms() -> Result<u64, HostError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("system clock predates Unix epoch"))?
        .as_millis();
    u64::try_from(now)
        .ok()
        .filter(|now| *now <= i64::MAX as u64)
        .ok_or_else(|| invalid("system clock exceeds SQLite range"))
}

impl TaskRuntime {
    pub(super) fn activate_scheduled(&mut self, host: &Host, now: u64) -> Result<(), HostError> {
        // A due timer wins over any signal the scheduler has yet to scan.
        self.journal.release_timers(now)?;
        let mut visited = std::collections::HashSet::new();
        for _ in 0..self.config.max_jobs_per_tick {
            let Some(wait) = self.journal.next_event_wait()? else {
                break;
            };
            if !visited.insert(wait.sequence) {
                break;
            }
            let page = host
                .store()
                .committed_page_after(wait.event_cursor, self.config.max_events_per_tick)
                .map_err(HostError::Store)?;
            let matching = page
                .iter()
                .find(|event| wait.activation.matches(&event.artifact));
            let cursor = matching
                .map(|event| event.sequence)
                .or_else(|| page.last().map(|event| event.sequence))
                .unwrap_or(wait.event_cursor);
            self.journal.record_event_scan(
                wait.sequence,
                cursor,
                matching.map(|event| &event.artifact.version_id),
                now,
            )?;
        }
        Ok(())
    }

    /// Begrenzt paginierte Aktivierungsbelege, einschließlich erfolgter Weckvorgänge.
    pub fn scheduled_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<ScheduledTask>, HostError> {
        self.journal.scheduled_after(sequence, limit)
    }

    /// Alle Aktivierungsrevisionen einer Aufgabe, einschließlich früherer Wartephasen.
    pub fn scheduled_history(
        &self,
        sequence: u64,
        after_revision: Option<u32>,
        limit: usize,
    ) -> Result<Vec<ScheduledTask>, HostError> {
        self.journal
            .scheduled_history(sequence, after_revision, limit)
    }
}

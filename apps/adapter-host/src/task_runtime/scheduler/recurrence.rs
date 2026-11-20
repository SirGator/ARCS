use arcs::core::VersionId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::SeriesState;
use crate::HostError;
use crate::task_runtime::{ReasoningRoute, TaskRuntime, TickReport, config::invalid};

const MAX_INTERVAL_MS: u64 = 315_576_000_000;
const CONSTRAINT: &str = "task_recurrence";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub every_ms: u64,
    pub max_occurrences: u32,
}

impl Recurrence {
    pub(in crate::task_runtime) fn validate(
        &self,
        route: &ReasoningRoute,
    ) -> Result<(), HostError> {
        if !(1..=MAX_INTERVAL_MS).contains(&self.every_ms)
            || !(2..=10_000).contains(&self.max_occurrences)
        {
            return Err(invalid(
                "recurrence requires every_ms in 1..=315576000000 and max_occurrences in 2..=10000",
            ));
        }
        if route.activation.is_some()
            || route.goal.is_some()
            || route.plan.is_some()
            || !route.dependencies.is_empty()
            || !route.allowed_capabilities.is_empty()
            || route.constraints.get(CONSTRAINT).is_some()
        {
            return Err(invalid(
                "recurrence requires an immediately eligible standalone Reasoning route without action authority",
            ));
        }
        Ok(())
    }

    pub(in crate::task_runtime) fn due(
        self,
        anchor: u64,
        occurrence: u32,
    ) -> Result<u64, HostError> {
        self.every_ms
            .checked_mul(u64::from(occurrence))
            .and_then(|offset| anchor.checked_add(offset))
            .filter(|due| *due <= i64::MAX as u64)
            .ok_or_else(|| invalid("recurrence timestamp exceeds SQLite range"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckSeries {
    /// Sequence of the first task in the series.
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub anchor_unix_ms: u64,
    pub every_ms: u64,
    pub max_occurrences: u32,
    pub next_due_unix_ms: Option<u64>,
    pub skipped_occurrences: u32,
    pub last_sequence: u64,
    pub last_state: String,
    #[serde(default)]
    pub state: SeriesState,
    #[serde(default)]
    pub control_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOccurrence {
    pub series_sequence: u64,
    /// Zero-based position on the original time grid.
    pub occurrence: u32,
    pub sequence: u64,
    pub due_unix_ms: u64,
    pub admitted_at_unix_ms: u64,
    pub skipped_before: u32,
}

pub(in crate::task_runtime) fn occurrence_route_id(series: u64, occurrence: u32) -> String {
    format!("check:{series}:{occurrence}")
}

impl TaskRuntime {
    pub(in crate::task_runtime) fn recurring_route(
        &self,
        sequence: u64,
        id: &str,
    ) -> Result<ReasoningRoute, HostError> {
        let (origin, occurrence) = self.journal.check_origin(sequence)?;
        let mut route = self
            .config
            .routes
            .iter()
            .find(|route| route.id == origin && route.recurrence.is_some())
            .cloned()
            .ok_or_else(|| invalid("recurring check has no authorized route"))?;
        if occurrence.occurrence == 0
            || id != occurrence_route_id(occurrence.series_sequence, occurrence.occurrence)
        {
            return Err(invalid("recurring check identity changed"));
        }
        route.id = id.into();
        Ok(route)
    }

    pub(in crate::task_runtime) fn recurring_constraints(
        &self,
        sequence: u64,
        constraints: &Value,
    ) -> Result<Value, HostError> {
        let (_, occurrence) = self.journal.check_origin(sequence)?;
        let evidence =
            serde_json::to_value(occurrence).map_err(|error| invalid(error.to_string()))?;
        if let Value::Object(fields) = constraints {
            let mut fields = fields.clone();
            fields.insert(CONSTRAINT.into(), evidence);
            Ok(Value::Object(fields))
        } else {
            Ok(json!({"operator_constraints": constraints, CONSTRAINT: evidence}))
        }
    }

    pub(in crate::task_runtime) fn admit_recurring_checks(
        &mut self,
        now: u64,
        report: &mut TickReport,
    ) -> Result<(), HostError> {
        report.recurring_jobs = self.journal.admit_recurring_checks(&self.config, now)?;
        Ok(())
    }

    pub fn recurring_checks_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<CheckSeries>, HostError> {
        self.journal.recurring_checks_after(sequence, limit)
    }

    pub fn check_occurrences_after(
        &self,
        series_sequence: u64,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<CheckOccurrence>, HostError> {
        self.journal
            .check_occurrences_after(series_sequence, after_sequence, limit)
    }
}

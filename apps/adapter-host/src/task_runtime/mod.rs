//! Persistente Reasoning-Routen und optionale, begrenzte Aufgabensteuerung.
mod config;
pub mod goal;
mod journal;
pub use config::{ReasoningRoute, RuntimeConfig};

use std::time::{Duration, Instant};

use arcs::core::{Artifact, VersionId};
use arcs::reasoning::{ContextSelection, ReasoningRequestV2, ReasoningStatus};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Host, HostError};
use config::invalid;
use journal::{Journal, db};

/// Dauerhafter Übergabepunkt an die fachliche Verification/Approval-Anwendung.
/// ready_for_review bedeutet nur: Reasoning ist abgeschlossen, nicht freigegeben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedTask {
    pub sequence: u64,
    pub route_id: String,
    pub input_version_id: VersionId,
    pub reasoning_status: ReasoningStatus,
    pub request_version_id: VersionId,
    pub result_version_id: VersionId,
    pub candidate_version_ids: Vec<VersionId>,
}
#[derive(Debug, Default, Serialize)]
pub struct TickReport {
    pub discovered_jobs: usize,
    pub backpressure: bool,
    pub ready_for_review: Vec<CompletedTask>,
}
#[derive(Debug, Serialize)]
pub struct RuntimeReport {
    pub runtime_id: String,
    pub ticks: u64,
    pub pending_jobs: usize,
    pub ready_for_review: u64,
    pub failed_jobs: u64,
    pub source_cursor: u64,
    pub completed_goals: u64,
    pub blocked_goals: u64,
    pub awaiting_approval: u64,
    pub goals: Vec<goal::GoalTask>,
}

pub struct TaskRuntime {
    config: RuntimeConfig,
    identity: Value,
    core_database: std::path::PathBuf,
    journal: Journal,
    scheduling_cursor: u64,
}
impl TaskRuntime {
    pub fn open(host: &Host, config: RuntimeConfig) -> Result<Self, HostError> {
        let identity = config.identity(host)?;
        let journal = Journal::open(host, &config, identity.clone())?;
        Ok(Self {
            config,
            identity,
            core_database: host.database_path().into(),
            journal,
            scheduling_cursor: 0,
        })
    }

    /// Pro Runde begrenzte Logseiten und Reasoning-Aufrufe; kein automatischer Retry.
    pub fn tick(&mut self, host: &mut Host) -> Result<TickReport, HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        let mut report = TickReport::default();
        let caught_up = self.discover(host, &mut report)?;
        // Erst alte Core-Ereignisse übernehmen. Bei voller Queue keine neuen ACKs.
        if caught_up
            && !report.backpressure
            && self.journal.pending()? < self.config.max_pending_jobs
        {
            host.poll_event_tick()?;
            self.discover(host, &mut report)?;
        } else if self.config.routes.iter().any(|route| route.goal.is_some()) {
            // Eine volle Aufgabenqueue darf Wirkungsbeobachtungen nicht anhalten.
            host.poll_round()?;
        }
        let mut visited = std::collections::HashSet::new();
        for _ in 0..self.config.max_jobs_per_tick {
            let Some((sequence, route_id, input_version_id)) =
                self.journal.next(self.scheduling_cursor)?
            else {
                break;
            };
            if !visited.insert(sequence) {
                break;
            }
            self.scheduling_cursor = sequence;
            let route = self
                .config
                .routes
                .iter()
                .find(|route| route.id == route_id)
                .ok_or_else(|| invalid("unknown saved route"))?;
            let input = host
                .store()
                .get(&input_version_id)
                .map_err(HostError::Store)?
                .ok_or_else(|| invalid("saved task input is missing"))?;
            if !matches_source(route, &input) {
                return Err(invalid("saved task no longer matches source"));
            }
            if let Some(goal) = &route.goal {
                let route = route.clone();
                let goal = goal.clone();
                if let Err(error) = self.advance_goal(host, sequence, &route, &goal, &input) {
                    self.journal.fail(sequence, &error)?;
                    return Err(error);
                }
                continue;
            }
            self.journal.start(sequence)?;
            let request = ReasoningRequestV2 {
                request_id: deterministic_invocation_id(
                    InvocationKind::Reasoning,
                    &[
                        "task-runtime",
                        &self.config.runtime_id,
                        &route.id,
                        &input_version_id.0,
                    ],
                ),
                capability: route.reasoner.clone(),
                method: route.method,
                objective: route.objective.clone(),
                context: vec![ContextSelection {
                    version_id: input_version_id.clone(),
                    payload_fields: route.payload_fields.clone(),
                }],
                target_schema_id: route.target_schema_id.clone(),
                allowed_capabilities: route.allowed_capabilities.clone(),
                constraints: route.constraints.clone(),
                budget: route.budget.clone(),
            };
            let result = match host.reason(request) {
                Ok(result) => result,
                Err(error) => {
                    self.journal.fail(sequence, &error)?;
                    return Err(error);
                }
            };
            let completed = CompletedTask {
                sequence,
                route_id,
                input_version_id,
                reasoning_status: result.status,
                request_version_id: result.request_version_id,
                result_version_id: result.result_version_id,
                candidate_version_ids: result
                    .candidates
                    .into_iter()
                    .map(|artifact| artifact.version_id)
                    .collect(),
            };
            // If this write is interrupted, the Core/adapter journal replays the same request.
            self.journal.complete(&completed)?;
            report.ready_for_review.push(completed);
        }
        Ok(report)
    }

    fn discover(&mut self, host: &Host, report: &mut TickReport) -> Result<bool, HostError> {
        let cursor: u64 = self
            .journal
            .connection
            .query_row("SELECT cursor FROM runtime_installation", [], |row| {
                row.get::<_, i64>(0).map(|value| value as u64)
            })
            .map_err(db)?;
        let page = host
            .store()
            .committed_page_after(cursor, self.config.max_events_per_tick)
            .map_err(HostError::Store)?;
        let transaction = self
            .journal
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut pending: usize = transaction
            .query_row(journal::PENDING_COUNT, [], |row| row.get::<_, i64>(0))
            .map(|value| value as usize)
            .map_err(db)?;
        for event in &page {
            let mut routes = Vec::new();
            for route in self
                .config
                .routes
                .iter()
                .filter(|route| matches_source(route, &event.artifact))
            {
                let existing: Option<i64> = transaction
                    .query_row(
                        "SELECT sequence FROM runtime_jobs WHERE route_id=?1 AND input_version=?2",
                        params![route.id, event.artifact.version_id.0],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(db)?;
                if existing.is_none() {
                    routes.push(route);
                }
            }
            if pending + routes.len() > self.config.max_pending_jobs {
                report.backpressure = true;
                break;
            }
            for route in routes {
                transaction.execute("INSERT INTO runtime_jobs(route_id,input_version,source_sequence,state) VALUES(?1,?2,?3,'pending')",params![route.id,event.artifact.version_id.0,event.sequence as i64]).map_err(db)?;
                pending += 1;
                report.discovered_jobs += 1;
            }
            // The cursor moves in the same transaction as every matching route job.
            transaction
                .execute(
                    "UPDATE runtime_installation SET cursor=?1",
                    [event.sequence as i64],
                )
                .map_err(db)?;
        }
        transaction.commit().map_err(db)?;
        Ok(!report.backpressure && page.len() < self.config.max_events_per_tick)
    }

    /// Die Anwendung hält ihren eigenen Review-Cursor. Es entstehen keine Actions.
    pub fn completed_after(
        &self,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<CompletedTask>, HostError> {
        if !(1..=10_000).contains(&limit) {
            return Err(invalid("review page size must be 1..10000"));
        }
        let mut statement = self.journal.connection.prepare("SELECT result_json FROM runtime_jobs WHERE sequence>?1 AND state='ready_for_review' ORDER BY sequence LIMIT ?2").map_err(db)?;
        let rows = statement
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(db)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(db)?)
                .map_err(|error| invalid(format!("invalid saved completion: {error}")))
        })
        .collect()
    }
    pub fn report(&self, ticks: u64) -> Result<RuntimeReport, HostError> {
        let count = |state: &str| {
            self.journal
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM runtime_jobs WHERE state=?1",
                    [state],
                    |row| row.get::<_, i64>(0).map(|value| value as u64),
                )
                .map_err(db)
        };
        Ok(RuntimeReport {
            runtime_id: self.config.runtime_id.clone(),
            ticks,
            pending_jobs: self.journal.pending()?,
            ready_for_review: count("ready_for_review")?,
            failed_jobs: count("failed")?,
            source_cursor: self
                .journal
                .connection
                .query_row("SELECT cursor FROM runtime_installation", [], |row| {
                    row.get::<_, i64>(0).map(|value| value as u64)
                })
                .map_err(db)?,
            completed_goals: count("completed")?,
            blocked_goals: count("blocked")?
                + count("rejected")?
                + count("budget_exhausted")?
                + count("cancelled")?,
            awaiting_approval: count("awaiting_approval")?,
            goals: self.goals_for_report()?,
        })
    }
    /// Stop wird zwischen Runden und während Wartepausen geprüft. Ein laufender
    /// stdio-Dialog bleibt durch seine konfigurierte Deadline begrenzt.
    pub fn run(
        &mut self,
        host: &mut Host,
        max_ticks: Option<u64>,
        mut stop: impl FnMut() -> bool,
    ) -> Result<RuntimeReport, HostError> {
        if max_ticks == Some(0) {
            return Err(invalid("max_ticks must be positive"));
        }
        let mut ticks = 0;
        while !stop() && max_ticks.is_none_or(|maximum| ticks < maximum) {
            self.tick(host)?;
            ticks += 1;
            if max_ticks.is_some_and(|maximum| ticks >= maximum) {
                break;
            }
            let until = Instant::now() + Duration::from_millis(self.config.interval_ms);
            while Instant::now() < until {
                if stop() {
                    return self.report(ticks);
                }
                std::thread::sleep(
                    until
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(25)),
                );
            }
        }
        self.report(ticks)
    }
}
fn matches_source(route: &ReasoningRoute, artifact: &Artifact) -> bool {
    artifact.created_by.id == route.source.adapter_id.0
        && artifact.schema_id == route.input_schema_id
        && artifact
            .tags
            .contains(&format!("capability:{}", route.source.capability_id.0))
}

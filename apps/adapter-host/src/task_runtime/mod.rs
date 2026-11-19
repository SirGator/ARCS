//! Persistente Reasoning-Routen und optionale, begrenzte Aufgabensteuerung.
mod config;
mod control;
pub mod deadlines;
pub mod dependencies;
pub mod goal;
mod journal;
pub mod planning;
pub mod reasoner_selection;
mod reasoning;
pub mod recovery;
pub mod scheduler;
pub mod suspension;
mod worker_lifecycle;
pub use config::{ReasoningRoute, RuntimeConfig};
pub use journal::dispatch::{DispatchClassReport, DispatchReport};

use std::time::{Duration, Instant};

use arcs::core::{Artifact, VersionId};
use arcs::reasoning::ReasoningStatus;
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
    pub failed: Vec<FailedTask>,
    pub waiting_resources: Vec<ResourceWait>,
    pub expired_tasks: Vec<u64>,
}
#[derive(Debug, Serialize)]
pub struct ResourceWait {
    pub sequence: u64,
    pub route_id: String,
    pub resource: String,
    pub requested: u64,
    pub available: u64,
}
#[derive(Debug, Serialize)]
pub struct FailedTask {
    pub sequence: u64,
    pub route_id: String,
    pub error: String,
}
#[derive(Debug, Serialize)]
pub struct RuntimeReport {
    pub paused_jobs: u64,
    pub task_controls: Vec<suspension::TaskControlStatus>,
    pub runtime_id: String,
    pub ticks: u64,
    pub pending_jobs: usize,
    pub ready_for_review: u64,
    pub failed_jobs: u64,
    pub cancelled_jobs: u64,
    pub source_cursor: u64,
    pub completed_goals: u64,
    pub blocked_goals: u64,
    pub awaiting_approval: u64,
    pub waiting_time: u64,
    pub waiting_event: u64,
    pub next_wake_unix_ms: Option<u64>,
    pub waiting_dependency: u64,
    pub blocked_dependencies: u64,
    pub capacity_used: usize,
    pub plans: Vec<planning::PlanReceipt>,
    pub planning_jobs: Vec<planning::PlannerJob>,
    pub running_plans: u64,
    pub stopped_plans: u64,
    pub review_plans: u64,
    pub goals: Vec<goal::GoalTask>,
    pub dispatch: DispatchReport,
    pub expired_jobs: u64,
    pub next_deadline_unix_ms: Option<u64>,
    pub task_deadlines: Vec<deadlines::TaskDeadline>,
}

pub struct TaskRuntime {
    config: RuntimeConfig,
    identity: Value,
    core_database: std::path::PathBuf,
    journal: Journal,
    control_cursor: u64,
    has_reasoner_selection: bool,
    has_reasoning_recovery: bool,
    live_clock: bool,
}
impl TaskRuntime {
    pub fn open(host: &Host, config: RuntimeConfig) -> Result<Self, HostError> {
        let identity = config.identity(host)?;
        let journal = Journal::open(host, &config, identity.clone())?;
        let has_reasoner_selection = config.routes.iter().any(|route| {
            route.reasoner_selection.is_some()
                || route.plan.as_ref().is_some_and(|plan| {
                    plan.templates
                        .iter()
                        .any(|template| template.route.reasoner_selection.is_some())
                })
        });
        let has_reasoning_recovery = config.routes.iter().any(|route| {
            route.reasoning_recovery.is_some()
                || route.plan.as_ref().is_some_and(|plan| {
                    plan.templates
                        .iter()
                        .any(|template| template.route.reasoning_recovery.is_some())
                })
        });
        Ok(Self {
            config,
            identity,
            core_database: host.database_path().into(),
            journal,
            control_cursor: 0,
            has_reasoner_selection,
            has_reasoning_recovery,
            live_clock: true,
        })
    }

    /// Pro Runde begrenzte Logseiten und Aufrufe; Recovery nur nach Betreiberpolicy.
    pub fn tick(&mut self, host: &mut Host) -> Result<TickReport, HostError> {
        self.tick_with_time(host, scheduler::now_unix_ms()?, true)
    }

    /// Vertrauenswürdige UTC-Zeit aus der Host-Anwendung; ermöglicht Simulationen
    /// langer Zeiträume ohne Änderungen an den Uhren externer Adapter.
    pub fn tick_at(&mut self, host: &mut Host, now_unix_ms: u64) -> Result<TickReport, HostError> {
        self.tick_with_time(host, now_unix_ms, false)
    }

    fn tick_with_time(
        &mut self,
        host: &mut Host,
        now_unix_ms: u64,
        live_clock: bool,
    ) -> Result<TickReport, HostError> {
        if now_unix_ms > i64::MAX as u64 {
            return Err(invalid("scheduler timestamp exceeds SQLite range"));
        }
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        host.poll_reconnections_at(now_unix_ms)?;
        self.live_clock = live_clock;
        let has_deadlines = self.config.routes.iter().any(|route| {
            route.max_lifetime_ms.is_some()
                || route.plan.as_ref().is_some_and(|plan| {
                    plan.templates
                        .iter()
                        .any(|template| template.route.max_lifetime_ms.is_some())
                })
        });
        if has_deadlines {
            self.journal.advance_deadline_clock(now_unix_ms)?;
        }
        let mut report = TickReport::default();
        if has_deadlines {
            self.expire_due_tasks(host, &mut report)?;
        }
        self.reap_abandoned_task_work(host)?;
        self.collect_paused_source_requests(host, has_deadlines, &mut report)?;
        self.collect_paused_reasoning(host, has_deadlines, live_clock, &mut report)?;
        self.cleanup_control_sessions(host)?;
        let caught_up = self.discover(host, &mut report, now_unix_ms)?;
        // Erst alte Core-Ereignisse übernehmen. Normale Inputs nur bei freier Queue.
        if caught_up
            && !report.backpressure
            && self.journal.capacity(self.config.retry_failed_jobs())?
                < self.config.max_pending_jobs
        {
            host.poll_event_tick()?;
            if has_deadlines && live_clock {
                self.journal
                    .advance_deadline_clock(scheduler::now_unix_ms()?)?;
            }
            self.discover(host, &mut report, now_unix_ms)?;
        } else {
            // Ein voller Aufgabenpuffer darf seine eigenen Weckereignisse nicht
            // aussperren. Nur beteiligte Input-Adapter werden weiter abgefragt.
            let wake_sources = self.journal.waiting_input_adapters()?;
            if !wake_sources.is_empty() {
                host.poll_wake_inputs(&wake_sources)?;
            }
            if !wake_sources.is_empty()
                || self.config.routes.iter().any(|route| {
                    route.goal.is_some()
                        || route.plan.as_ref().is_some_and(|plan| {
                            plan.templates
                                .iter()
                                .any(|template| template.route.goal.is_some())
                        })
                })
            {
                host.poll_round()?;
            }
        }
        self.activate_scheduled(host, now_unix_ms)?;
        self.advance_dependencies(host)?;
        self.journal.begin_dispatch_tick()?;
        for _ in 0..self.config.max_jobs_per_tick {
            let Some((sequence, route_id, input_version_id, priority)) =
                self.journal.next(self.config.retry_failed_jobs())?
            else {
                break;
            };
            // The bounded expiry sweep may not have reached this job yet.
            // Check again immediately before any per-task Core or adapter work.
            if has_deadlines && live_clock {
                self.journal
                    .advance_deadline_clock(scheduler::now_unix_ms()?)?;
            }
            if has_deadlines && self.expire_task(host, sequence, &mut report)? {
                continue;
            }
            let route = self.raw_route_for_job(host, sequence, &route_id)?;
            if route.priority != priority {
                return Err(invalid(
                    "saved task priority differs from its authorized route",
                ));
            }
            let input = host
                .store()
                .get(&input_version_id)
                .map_err(HostError::Store)?
                .ok_or_else(|| invalid("saved task input is missing"))?;
            if !matches_source(&route, &input) {
                return Err(invalid("saved task no longer matches source"));
            }
            match self.advance_job(host, sequence, &route, &input, now_unix_ms) {
                Ok(Some(completed)) => {
                    self.journal.complete(&completed)?;
                    report.ready_for_review.push(completed);
                }
                Ok(None) => {}
                Err(error) => self.record_job_failure(sequence, &route, error, &mut report)?,
            }
        }
        self.cleanup_control_sessions(host)?;
        Ok(report)
    }

    fn discover(
        &mut self,
        host: &Host,
        report: &mut TickReport,
        now: u64,
    ) -> Result<bool, HostError> {
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
            .query_row(
                &journal::capacity_query(self.config.retry_failed_jobs()),
                [],
                |row| row.get::<_, i64>(0),
            )
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
            let slots: usize = routes.iter().map(|route| route.queue_slots()).sum();
            if pending + slots > self.config.max_pending_jobs {
                report.backpressure = true;
                break;
            }
            for route in &routes {
                transaction.execute("INSERT INTO runtime_jobs(route_id,input_version,source_sequence,state) VALUES(?1,?2,?3,'pending')",params![route.id,event.artifact.version_id.0,event.sequence as i64]).map_err(db)?;
                let sequence = transaction.last_insert_rowid() as u64;
                Journal::assign_priority(&transaction, sequence, route.priority)?;
                Journal::assign_deadline(
                    &transaction,
                    sequence,
                    route.max_lifetime_ms,
                    route.plan.is_some(),
                    None,
                )?;
                Journal::reserve_plan(&transaction, sequence, route.queue_slots())?;
                if let Some(activation) = &route.activation {
                    Journal::schedule(&transaction, sequence, event.sequence, activation, now)?;
                }
                pending += route.queue_slots();
                report.discovered_jobs += 1;
            }
            Journal::bind_dependencies(&transaction, &routes, &event.artifact.version_id)?;
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
        let (waiting_time, waiting_event, next_wake_unix_ms) = self.journal.schedule_summary()?;
        let (waiting_dependency, blocked_dependencies) = self.journal.dependency_summary()?;
        let (running_plans, stopped_plans, review_plans) = self.journal.plan_summary()?;
        let (expired_jobs, next_deadline_unix_ms) = self
            .journal
            .deadline_summary(self.config.retry_failed_jobs())?;
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
        let cancelled_goals: u64 = self.journal.connection.query_row(
            "SELECT COUNT(*) FROM runtime_jobs WHERE state='cancelled' AND goal_json IS NOT NULL",
            [], |row| row.get::<_, i64>(0).map(|n| n as u64),
        ).map_err(db)?;
        Ok(RuntimeReport {
            paused_jobs: self.journal.paused_count()?,
            task_controls: self.journal.task_controls_report()?,
            runtime_id: self.config.runtime_id.clone(),
            ticks,
            pending_jobs: self.journal.pending(self.config.retry_failed_jobs())?,
            ready_for_review: count("ready_for_review")?,
            failed_jobs: count("failed")?,
            cancelled_jobs: count("cancelled")?,
            source_cursor: self
                .journal
                .connection
                .query_row("SELECT cursor FROM runtime_installation", [], |row| {
                    row.get::<_, i64>(0).map(|value| value as u64)
                })
                .map_err(db)?,
            completed_goals: count("completed")?,
            blocked_goals: self.journal.blocked_goal_count()?
                + count("rejected")?
                + count("budget_exhausted")?
                + cancelled_goals,
            awaiting_approval: count("awaiting_approval")?,
            waiting_time,
            waiting_event,
            next_wake_unix_ms,
            waiting_dependency,
            blocked_dependencies,
            capacity_used: self.journal.capacity(self.config.retry_failed_jobs())?,
            plans: self.journal.plans_for_report()?,
            planning_jobs: self.journal.planning_jobs_after(0, 50)?,
            running_plans,
            stopped_plans,
            review_plans,
            goals: self.goals_for_report()?,
            dispatch: self.journal.dispatch_report()?,
            expired_jobs,
            next_deadline_unix_ms,
            task_deadlines: self.journal.deadlines_for_report()?,
        })
    }
    /// Stop wird zwischen Runden und während Wartepausen geprüft. Paralleles
    /// Reasoning wartet dabei nicht auf Antworten; aktive Worker werden beim
    /// Host-Shutdown abgebrochen. Andere synchrone Ports behalten ihre Deadline.
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
    matches_capability(&route.source, &route.input_schema_id, artifact)
}
fn matches_capability(
    source: &arcs::adapters::CapabilityRef,
    schema: &arcs::core::SchemaId,
    artifact: &Artifact,
) -> bool {
    artifact.created_by.id == source.adapter_id.0
        && &artifact.schema_id == schema
        && artifact
            .tags
            .contains(&format!("capability:{}", source.capability_id.0))
}

// Persistence/identity failures remain fatal. Isolating a component failure
// must never hide a broken Core store or a changed topology.
fn is_core_failure(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Store(_)
            | HostError::Reduction(_)
            | HostError::Input {
                source: arcs::input::InputError::Store(_),
                ..
            }
            | HostError::Registration(arcs::adapters::AdapterRegistryError::Store(_))
            | HostError::Request {
                source: arcs::request::RequestError::Store(_)
                    | arcs::request::RequestError::Invocation(_)
                    | arcs::request::RequestError::StaleDispatch,
                ..
            }
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Store(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Invocation(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Retrieval(
                arcs::store::retrieval::RetrievalError::Store(_)
            ))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::InputData(
                arcs::adapters::input_data::InputDataError::Store(_)
            ))
    )
}

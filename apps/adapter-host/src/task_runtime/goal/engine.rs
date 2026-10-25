use arcs::adapters::input_data::InputDataSelection;
use arcs::adapters::{AdapterId, CapabilityId};
use arcs::core::{Artifact, SchemaId, SystemClock, VersionId};
use arcs::input::{InputMessage, InputService};
use arcs::outcome::OutcomeVerdict;
use arcs::reasoning::{ContextSelection, ReasoningRequestV2, ReasoningStatus};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use rusqlite::{OptionalExtension, params};

use super::*;
use crate::config::RUNTIME_ID;
use crate::task_runtime::{TaskRuntime, journal::db};

impl TaskRuntime {
    pub(in crate::task_runtime) fn goals_for_report(&self) -> Result<Vec<GoalTask>, HostError> {
        let mut query = self.journal.connection.prepare("SELECT goal_json FROM runtime_jobs WHERE goal_json IS NOT NULL ORDER BY (state IN ('pending','running','failed','awaiting_approval')) DESC,sequence DESC LIMIT 50").map_err(db)?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(db)?)
                .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
        })
        .collect()
    }
    pub fn goals_after(&self, sequence: u64, limit: usize) -> Result<Vec<GoalTask>, HostError> {
        if !(1..=1000).contains(&limit) {
            return Err(invalid("goal page size must be 1..1000"));
        }
        let mut query = self.journal.connection.prepare("SELECT goal_json FROM runtime_jobs WHERE sequence>?1 AND goal_json IS NOT NULL ORDER BY sequence LIMIT ?2").map_err(db)?;
        let rows = query
            .query_map(
                params![sequence.min(i64::MAX as u64) as i64, limit as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(db)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(db)?)
                .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
        })
        .collect()
    }

    pub(super) fn saved_goal(&self, sequence: u64) -> Result<Option<GoalTask>, HostError> {
        let value: Option<Option<String>> = self
            .journal
            .connection
            .query_row(
                "SELECT goal_json FROM runtime_jobs WHERE sequence=?1",
                [sequence.min(i64::MAX as u64) as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(db)?;
        value
            .flatten()
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| invalid(format!("invalid goal checkpoint: {e}")))
            })
            .transpose()
    }

    pub(super) fn save_goal(&self, task: &GoalTask) -> Result<(), HostError> {
        let json = serde_json::to_string(task).map_err(|e| invalid(e.to_string()))?;
        self.journal
            .connection
            .execute(
                "UPDATE runtime_jobs SET state=?2,goal_json=?3,error=NULL WHERE sequence=?1",
                params![task.sequence as i64, task.state.queue_state(), json],
            )
            .map_err(db)?;
        Ok(())
    }

    pub(in crate::task_runtime) fn advance_goal(
        &mut self,
        host: &mut Host,
        sequence: u64,
        route: &ReasoningRoute,
        config: &GoalConfig,
        input: &Artifact,
    ) -> Result<(), HostError> {
        let mut task = match self.saved_goal(sequence)? {
            Some(task) => task,
            None => {
                let input_cursor: i64 = self
                    .journal
                    .connection
                    .query_row(
                        "SELECT source_sequence FROM runtime_jobs WHERE sequence=?1",
                        [sequence as i64],
                        |row| row.get(0),
                    )
                    .map_err(db)?;
                let task = GoalTask {
                    sequence,
                    route_id: route.id.clone(),
                    input_version_id: input.version_id.clone(),
                    state: GoalState::Acquiring,
                    round: 1,
                    observation_cursor: input_cursor as u64,
                    dispatch_cursor: None,
                    wait_ticks: 0,
                    source_cursors: BTreeMap::new(),
                    source_versions: BTreeMap::new(),
                    before: vec![],
                    after: vec![],
                    candidate: None,
                    capability: None,
                    verification: None,
                    approval: None,
                    action: None,
                    execution: None,
                    assessment: None,
                    outcome: None,
                    detail: String::new(),
                };
                // Persist the freshness barrier before any acquisition can be retried.
                self.save_goal(&task)?;
                task
            }
        };
        match task.state {
            GoalState::Acquiring | GoalState::Observing => {
                let after = task.state == GoalState::Observing;
                if let Some(versions) = self.acquire(host, route, config, &mut task, after)? {
                    task.wait_ticks = 0;
                    task.source_cursors.clear();
                    task.source_versions.clear();
                    if after {
                        task.after = versions;
                        task.state = GoalState::Assessing;
                    } else {
                        task.before = versions;
                        task.state = if config.initial_assessment {
                            GoalState::Checking
                        } else {
                            GoalState::Planning
                        };
                    }
                } else {
                    task.wait_ticks += 1;
                    if task.wait_ticks >= config.max_wait_ticks {
                        task.state = GoalState::Blocked;
                        task.detail =
                            "fresh observations did not arrive within the configured wait budget"
                                .into();
                    }
                }
            }
            GoalState::Checking => {
                let mut context = vec![ContextSelection {
                    version_id: input.version_id.clone(),
                    payload_fields: route.payload_fields.clone(),
                }];
                append_sources(&mut context, config, &task.before);
                let assessment = &config.assessment;
                let result = host.reason_with_data(
                    ReasoningRequestV2 {
                        request_id: self.goal_request_id(&task, "check"),
                        capability: assessment.capability.clone(),
                        method: assessment.method,
                        objective: assessment.objective.clone(),
                        context,
                        target_schema_id: SchemaId(ASSESSMENT_SCHEMA.into()),
                        allowed_capabilities: vec![],
                        constraints: goal_constraints(
                            &assessment.constraints,
                            config,
                            &task,
                            "check",
                        ),
                        budget: assessment.budget.clone(),
                    },
                    &binary_inputs(config, &task.before),
                )?;
                if result.status != ReasoningStatus::Solved || result.candidates.len() != 1 {
                    task.state = GoalState::Blocked;
                    task.detail = reasoning_explanation(
                        host,
                        &result,
                        "initial goal assessment unavailable",
                    )?;
                } else {
                    let candidate = &result.candidates[0];
                    let verdict: GoalAssessment = serde_json::from_value(candidate.payload.clone())
                        .map_err(|e| invalid(format!("invalid goal assessment: {e}")))?;
                    let allowed: HashSet<_> =
                        task.before.iter().chain([&input.version_id]).collect();
                    validate_assessment_evidence(&verdict, &allowed, &task.before)?;
                    task.assessment = Some(candidate.version_id.clone());
                    task.detail = verdict.detail;
                    task.state = match (verdict.goal_status, verdict.effect) {
                        (
                            GoalVerdict::Achieved,
                            OutcomeVerdict::Unknown | OutcomeVerdict::Success,
                        ) => GoalState::Completed,
                        (GoalVerdict::Continue, OutcomeVerdict::Unknown) => GoalState::Planning,
                        _ => GoalState::Blocked,
                    };
                }
            }
            GoalState::Planning => {
                let mut context = vec![ContextSelection {
                    version_id: input.version_id.clone(),
                    payload_fields: route.payload_fields.clone(),
                }];
                append_sources(&mut context, config, &task.before);
                if let Some(previous) = &task.assessment {
                    context.push(ContextSelection {
                        version_id: previous.clone(),
                        payload_fields: assessment_fields(),
                    });
                }
                let request = ReasoningRequestV2 {
                    request_id: self.goal_request_id(&task, "plan"),
                    capability: route.reasoner.clone(),
                    method: route.method,
                    objective: route.objective.clone(),
                    context,
                    target_schema_id: route.target_schema_id.clone(),
                    allowed_capabilities: route.allowed_capabilities.clone(),
                    constraints: goal_constraints(&route.constraints, config, &task, "plan"),
                    budget: route.budget.clone(),
                };
                let result =
                    host.reason_with_data(request, &binary_inputs(config, &task.before))?;
                if result.status != ReasoningStatus::Solved || result.candidates.len() != 1 {
                    task.state = GoalState::Blocked;
                    task.detail = reasoning_explanation(
                        host,
                        &result,
                        &format!(
                            "planner returned {:?}; no executable proposal",
                            result.status
                        ),
                    )?;
                } else {
                    let candidate = &result.candidates[0];
                    let capabilities: Vec<_> = config
                        .actions
                        .iter()
                        .filter(|action| {
                            candidate.tags.contains(&format!(
                                "requires:{}:{}",
                                action.capability.adapter_id.0, action.capability.capability_id.0
                            ))
                        })
                        .collect();
                    let required = candidate
                        .tags
                        .iter()
                        .filter(|tag| tag.starts_with("requires:"))
                        .count();
                    if capabilities.len() != 1 || required != 1 {
                        task.state = GoalState::Blocked;
                        task.detail =
                            "planner must propose exactly one configured action capability".into();
                    } else {
                        task.candidate = Some(candidate.version_id.clone());
                        task.capability = Some(capabilities[0].capability.clone());
                        task.verification = None;
                        task.approval = None;
                        task.action = None;
                        task.execution = None;
                        task.outcome = None;
                        task.dispatch_cursor = None;
                        task.state = GoalState::Verifying;
                    }
                }
            }
            GoalState::Verifying => self.verify_goal(host, route, config, &mut task)?,
            GoalState::AwaitingApproval => self.recover_goal_review(host, &mut task)?,
            GoalState::Executing => {
                if task.dispatch_cursor.is_none() {
                    let cursor = host.store().latest_sequence().map_err(HostError::Store)?;
                    task.dispatch_cursor = Some(cursor);
                    task.observation_cursor = cursor;
                    self.save_goal(&task)?;
                }
                self.execute_goal(host, route, config, &mut task)?;
                task.wait_ticks = 0;
                task.state = GoalState::Observing;
            }
            GoalState::Assessing => {
                let candidate = load(
                    host,
                    task.candidate
                        .as_ref()
                        .ok_or_else(|| invalid("missing goal candidate"))?,
                )?;
                let execution = task
                    .execution
                    .as_ref()
                    .ok_or_else(|| invalid("missing goal execution"))?;
                let mut context = vec![
                    ContextSelection {
                        version_id: input.version_id.clone(),
                        payload_fields: route.payload_fields.clone(),
                    },
                    ContextSelection {
                        version_id: candidate.version_id.clone(),
                        payload_fields: payload_fields(&candidate)?,
                    },
                    ContextSelection {
                        version_id: execution.clone(),
                        payload_fields: config.execution_payload_fields.clone(),
                    },
                ];
                append_sources(&mut context, config, &task.before);
                append_sources(&mut context, config, &task.after);
                let assessment = &config.assessment;
                let result = host.reason_with_data(
                    ReasoningRequestV2 {
                        request_id: self.goal_request_id(&task, "assess"),
                        capability: assessment.capability.clone(),
                        method: assessment.method,
                        objective: assessment.objective.clone(),
                        context,
                        target_schema_id: SchemaId(ASSESSMENT_SCHEMA.into()),
                        allowed_capabilities: vec![],
                        constraints: goal_constraints(
                            &assessment.constraints,
                            config,
                            &task,
                            "assess",
                        ),
                        budget: assessment.budget.clone(),
                    },
                    &binary_inputs(config, &task.after),
                )?;
                if result.status != ReasoningStatus::Solved || result.candidates.len() != 1 {
                    task.state = GoalState::Blocked;
                    task.detail =
                        "effect assessment unavailable; execution is not proof of goal completion"
                            .into();
                } else {
                    let artifact = &result.candidates[0];
                    let verdict: GoalAssessment = serde_json::from_value(artifact.payload.clone())
                        .map_err(|e| invalid(format!("invalid goal assessment: {e}")))?;
                    let allowed: HashSet<_> = task
                        .before
                        .iter()
                        .chain(&task.after)
                        .chain([&input.version_id, &candidate.version_id, execution])
                        .collect();
                    validate_assessment_evidence(&verdict, &allowed, &task.after)?;
                    task.assessment = Some(artifact.version_id.clone());
                    task.outcome = Some(self.record_goal_outcome(host, &task, &verdict)?);
                    task.detail = verdict.detail;
                    task.state = match (verdict.goal_status, verdict.effect) {
                        (GoalVerdict::Achieved, OutcomeVerdict::Success) => GoalState::Completed,
                        (
                            GoalVerdict::Continue,
                            OutcomeVerdict::Success | OutcomeVerdict::Failure,
                        ) if task.round < config.max_rounds => {
                            task.round += 1;
                            task.before = task.after.clone();
                            task.after.clear();
                            GoalState::Planning
                        }
                        (GoalVerdict::Continue, _) if task.round >= config.max_rounds => {
                            GoalState::BudgetExhausted
                        }
                        _ => GoalState::Blocked,
                    };
                }
            }
            _ => return Ok(()),
        }
        self.save_goal(&task)
    }

    fn goal_request_id(&self, task: &GoalTask, phase: &str) -> String {
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                "goal-runtime",
                &self.config.runtime_id,
                &task.route_id,
                &task.input_version_id.0,
                &task.round.to_string(),
                phase,
            ],
        )
    }

    fn acquire(
        &mut self,
        host: &mut Host,
        route: &ReasoningRoute,
        config: &GoalConfig,
        task: &mut GoalTask,
        after: bool,
    ) -> Result<Option<Vec<VersionId>>, HostError> {
        let mut versions = Vec::new();
        for source in &config.sources {
            if let Some(version) = task.source_versions.get(&source.id) {
                versions.push(version.clone());
                continue;
            }
            let artifact = if let Some(query) = &source.query {
                let key = deterministic_invocation_id(
                    InvocationKind::Request,
                    &[
                        "goal-source",
                        &self.config.runtime_id,
                        &route.id,
                        &task.input_version_id.0,
                        &task.round.to_string(),
                        if after { "after" } else { "before" },
                        &source.id,
                    ],
                );
                let query = InputService::new(
                    &self.journal.operator,
                    host.schemas(),
                    host.store(),
                    &mut self.journal.ids,
                    &SystemClock,
                )
                .ingest(
                    &AdapterId(RUNTIME_ID.into()),
                    InputMessage {
                        capability_id: CapabilityId(format!(
                            "runtime.query.{}.{}",
                            route.id, source.id
                        )),
                        external_subject: Some(task.input_version_id.0.clone()),
                        external_reference: key,
                        payload: query.payload.clone(),
                    },
                )
                .map_err(|source| HostError::Input {
                    request: route.id.clone(),
                    source,
                })?;
                host.request_version(&source.capability, &query.version_id, &source.schema_id)?
            } else {
                // Bounded pages avoid an unbounded history load. Retain the first
                // matching immutable snapshot, so retries select the same evidence.
                let mut cursor = *task
                    .source_cursors
                    .get(&source.id)
                    .unwrap_or(&task.observation_cursor);
                let mut found = None;
                for _ in 0..4 {
                    let page = host
                        .store()
                        .committed_page_after(cursor, self.config.max_events_per_tick)
                        .map_err(HostError::Store)?;
                    let done = page.len() < self.config.max_events_per_tick;
                    for event in page {
                        cursor = event.sequence;
                        let artifact = event.artifact;
                        if artifact.created_by.id == source.capability.adapter_id.0
                            && artifact.schema_id == source.schema_id
                            && artifact.tags.contains(&format!(
                                "capability:{}",
                                source.capability.capability_id.0
                            ))
                            && source.subject_id.as_ref().is_none_or(|subject| {
                                artifact
                                    .subject
                                    .as_ref()
                                    .is_some_and(|actual| &actual.0 == subject)
                            })
                        {
                            found = Some(artifact);
                            break;
                        }
                    }
                    if found.is_some() || done {
                        break;
                    }
                }
                task.source_cursors.insert(source.id.clone(), cursor);
                let Some(artifact) = found else {
                    self.save_goal(task)?;
                    return Ok(None);
                };
                artifact
            };
            task.source_versions
                .insert(source.id.clone(), artifact.version_id.clone());
            self.save_goal(task)?;
            versions.push(artifact.version_id);
        }
        Ok(Some(versions))
    }
}

fn append_sources(
    context: &mut Vec<ContextSelection>,
    config: &GoalConfig,
    versions: &[VersionId],
) {
    for (source, version) in config.sources.iter().zip(versions) {
        if !context
            .iter()
            .any(|selection| &selection.version_id == version)
        {
            context.push(ContextSelection {
                version_id: version.clone(),
                payload_fields: source.payload_fields.clone(),
            });
        }
    }
}
fn binary_inputs(config: &GoalConfig, versions: &[VersionId]) -> Vec<InputDataSelection> {
    config
        .sources
        .iter()
        .zip(versions)
        .filter_map(|(source, version)| {
            source.binary.as_ref().map(|binary| InputDataSelection {
                name: binary.name.clone(),
                source: binary.source.clone(),
                source_version_id: version.clone(),
                reference_pointer: binary.reference_pointer.clone(),
            })
        })
        .collect()
}
fn goal_constraints(
    operator: &serde_json::Value,
    config: &GoalConfig,
    task: &GoalTask,
    phase: &str,
) -> serde_json::Value {
    let sources = |versions: &[VersionId]| {
        config
            .sources
            .iter()
            .zip(versions)
            .map(|(source, version)| (source.id.clone(), version.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    serde_json::json!({"operator_constraints":operator,"goal_runtime":{
        "phase":phase,"round":task.round,"task_version_id":task.input_version_id,
        "before":sources(&task.before),"after":sources(&task.after),
        "candidate_version_id":task.candidate,"execution_version_id":task.execution,
        "previous_assessment_version_id":task.assessment
    }})
}
pub(super) fn assessment_fields() -> Vec<String> {
    ["goal_status", "effect", "detail", "evidence_version_ids"]
        .map(str::to_owned)
        .to_vec()
}
fn validate_assessment_evidence(
    verdict: &GoalAssessment,
    allowed: &HashSet<&VersionId>,
    required: &[VersionId],
) -> Result<(), HostError> {
    let evidence: HashSet<_> = verdict.evidence_version_ids.iter().collect();
    if verdict.detail.trim().is_empty()
        || verdict.detail.len() > 2000
        || verdict.evidence_version_ids.len() > 32
        || evidence.len() != verdict.evidence_version_ids.len()
        || !evidence.iter().all(|version| allowed.contains(version))
        || !required.iter().all(|version| evidence.contains(version))
    {
        return Err(invalid(
            "assessment must reference every fresh source and only supplied evidence",
        ));
    }
    Ok(())
}
fn reasoning_explanation(
    host: &Host,
    result: &arcs::reasoning::general::ReasoningResult,
    fallback: &str,
) -> Result<String, HostError> {
    let artifact = load(host, &result.result_version_id)?;
    Ok(artifact
        .payload
        .pointer("/response/trace/details/explanation")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(|text| text.chars().take(1000).collect())
        .unwrap_or_else(|| fallback.into()))
}
pub(super) fn payload_fields(artifact: &Artifact) -> Result<Vec<String>, HostError> {
    artifact
        .payload
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .ok_or_else(|| invalid("goal candidate must have an object payload"))
}
pub(super) fn load(host: &Host, version: &VersionId) -> Result<Artifact, HostError> {
    host.store()
        .get(version)
        .map_err(HostError::Store)?
        .ok_or_else(|| invalid("missing goal artifact"))
}

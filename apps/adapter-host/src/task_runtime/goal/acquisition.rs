//! Bounded source acquisition; immutable evidence is committed only by the Host.
use super::*;
use crate::config::RUNTIME_ID;
use crate::task_runtime::TaskRuntime;
use arcs::adapters::AdapterId;
use arcs::core::SystemClock;
use arcs::input::{InputMessage, InputService};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};

pub(super) enum Acquisition {
    Complete(Vec<VersionId>),
    PendingRequests,
    WaitingObservations,
}
impl TaskRuntime {
    pub(super) fn acquire(
        &mut self,
        host: &mut Host,
        route: &ReasoningRoute,
        config: &GoalConfig,
        task: &mut GoalTask,
        after: bool,
    ) -> Result<Acquisition, HostError> {
        let mut versions = Vec::new();
        let mut pending = false;
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
                let query_capability =
                    self.journal
                        .goal_query_capability(task.sequence, &route.id, &source.id)?;
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
                        capability_id: query_capability,
                        external_subject: Some(task.input_version_id.0.clone()),
                        external_reference: key,
                        payload: query.payload.clone(),
                    },
                )
                .map_err(|source| HostError::Input {
                    request: route.id.clone(),
                    source,
                })?;
                self.journal
                    .bind_source_request(task.sequence, task.round, &query.version_id)?;
                let Some(artifact) = host.poll_request_version(
                    &source.capability,
                    &query.version_id,
                    &source.schema_id,
                    self.config.max_concurrent_requests,
                )?
                else {
                    pending = true;
                    continue;
                };
                artifact
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
                    continue;
                };
                artifact
            };
            task.source_versions
                .insert(source.id.clone(), artifact.version_id.clone());
            self.save_goal(task)?;
            versions.push(artifact.version_id);
        }
        Ok(if versions.len() == config.sources.len() {
            Acquisition::Complete(versions)
        } else if pending {
            Acquisition::PendingRequests
        } else {
            Acquisition::WaitingObservations
        })
    }
}

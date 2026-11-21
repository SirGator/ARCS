//! Bounded source acquisition; immutable evidence is committed only by the Host.
use super::*;
use crate::config::RUNTIME_ID;
use crate::task_runtime::TaskRuntime;
use arcs::adapters::AdapterId;
use arcs::core::SystemClock;
use arcs::input::{InputMessage, InputService};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use arcs::store::{ArtifactRelations, relation_kinds};

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
                let (payload, parents) = self.bind_query_payload(host, task, query)?;
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
                        payload,
                    },
                )
                .map_err(|source| HostError::Input {
                    request: route.id.clone(),
                    source,
                })?;
                let relations = ArtifactRelations::new(host.store());
                for parent in parents {
                    let kind = relation_kinds::based_on();
                    if !relations
                        .outgoing(&query.version_id)
                        .map_err(HostError::Store)?
                        .iter()
                        .any(|edge| edge.to == parent && edge.kind == kind)
                    {
                        relations
                            .connect(query.version_id.clone(), parent, kind)
                            .map_err(HostError::Store)?;
                    }
                }
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

    fn bind_query_payload(
        &self,
        host: &Host,
        task: &GoalTask,
        query: &GoalQuery,
    ) -> Result<(Value, Vec<VersionId>), HostError> {
        let mut payload = query.payload.clone();
        let mut parents = Vec::new();
        if query.from_dependencies.is_empty() {
            return Ok((payload, parents));
        }
        let mut selections = BTreeMap::<String, HashSet<String>>::new();
        for item in self.journal.dependency_context(task.sequence)? {
            selections
                .entry(item.version_id.0)
                .or_default()
                .extend(item.payload_fields);
        }
        let evidence = selections
            .into_iter()
            .map(|(version, fields)| {
                let artifact = host
                    .store()
                    .get(&VersionId(version))
                    .map_err(HostError::Store)?
                    .ok_or_else(|| invalid("goal query dependency version is missing"))?;
                Ok((artifact, fields))
            })
            .collect::<Result<Vec<_>, HostError>>()?;
        for (target, binding) in &query.from_dependencies {
            let root = binding
                .root_field()
                .ok_or_else(|| invalid("invalid goal query pointer"))?;
            let mut available = evidence.iter().filter(|(artifact, fields)| {
                artifact.schema_id == binding.schema_id && fields.contains(root)
            });
            let (artifact, _) = available
                .next()
                .ok_or_else(|| invalid("goal query dependency field is not exported"))?;
            if available.next().is_some() {
                return Err(invalid("goal query dependency field is ambiguous"));
            }
            let value = artifact
                .payload
                .pointer(&binding.pointer)
                .ok_or_else(|| invalid("goal query dependency pointer is missing"))?;
            let slot = payload
                .get_mut(target)
                .ok_or_else(|| invalid("goal query target field is missing"))?;
            *slot = value.clone();
            if !parents.contains(&artifact.version_id) {
                parents.push(artifact.version_id.clone());
            }
        }
        Ok((payload, parents))
    }
}

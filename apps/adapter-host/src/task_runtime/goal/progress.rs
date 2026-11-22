//! Assessed rounds are immutable Core artifacts; the runtime keeps their head.
use arcs::adapters::{AdapterId, CapabilityId};
use arcs::core::{SystemClock, VersionId};
use arcs::input::{InputMessage, InputService};
use arcs::runtime::{InvocationKind, deterministic_invocation_id};
use arcs::store::{ArtifactRelations, relation_kinds};
use serde_json::{Value, json};

use super::{GoalAssessment, GoalState, GoalTask, ROUND_CAPABILITY, invalid};
use crate::config::RUNTIME_ID;
use crate::task_runtime::TaskRuntime;
use crate::{Host, HostError};

impl TaskRuntime {
    pub(super) fn record_goal_round(
        &mut self,
        host: &Host,
        task: &GoalTask,
        assessment: &GoalAssessment,
        next: GoalState,
    ) -> Result<VersionId, HostError> {
        let (phase, outcome) =
            match task.state {
                GoalState::Checking => ("check", None),
                GoalState::Assessing => (
                    "assess",
                    Some(task.outcome.as_ref().ok_or_else(|| {
                        invalid("assessed effect is missing its outcome version")
                    })?),
                ),
                _ => return Err(invalid("a round requires a validated assessment")),
            };
        let assessment_version = task
            .assessment
            .as_ref()
            .ok_or_else(|| invalid("assessed round is missing its assessment version"))?;
        let definition = self.journal.definition_version()?;
        let subject = deterministic_invocation_id(
            InvocationKind::Request,
            &[
                "goal-round-history",
                &definition.0,
                &task.sequence.to_string(),
                &task.input_version_id.0,
            ],
        );
        let reference = deterministic_invocation_id(
            InvocationKind::Request,
            &["goal-round", &subject, &task.round.to_string()],
        );
        let mut payload = json!({
            "runtime_definition_version_id": definition,
            "task_sequence": task.sequence,
            "input_version_id": task.input_version_id,
            "round": task.round,
            "phase": phase,
            "goal_status": assessment.goal_status,
            "effect": assessment.effect,
            "next_state": next,
            "assessment_version_id": assessment_version,
        });
        if let Some(outcome) = outcome {
            payload["outcome_version_id"] = Value::String(outcome.0.clone());
        }
        if let Some(previous) = &task.latest_round_version_id {
            payload["previous_round_version_id"] = Value::String(previous.0.clone());
        }
        // Replaying before the runtime checkpoint must recover the same round.
        let artifact = InputService::new(
            &self.journal.operator,
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(RUNTIME_ID.into()),
            InputMessage {
                capability_id: CapabilityId(ROUND_CAPABILITY.into()),
                external_subject: Some(subject),
                external_reference: reference,
                payload,
            },
        )
        .map_err(|source| HostError::Input {
            request: "goal round publication".into(),
            source,
        })?;
        let links = ArtifactRelations::new(host.store());
        let saved = links
            .outgoing(&artifact.version_id)
            .map_err(HostError::Store)?;
        let parents = [
            (&task.input_version_id, relation_kinds::part_of_goal()),
            (&definition, relation_kinds::based_on()),
            (assessment_version, relation_kinds::supported_by()),
        ]
        .into_iter()
        .chain(
            outcome
                .into_iter()
                .map(|version| (version, relation_kinds::supported_by())),
        )
        .chain(
            task.latest_round_version_id
                .iter()
                .map(|version| (version, relation_kinds::based_on())),
        );
        for (version, kind) in parents {
            if !saved
                .iter()
                .any(|edge| &edge.to == version && edge.kind == kind)
            {
                links
                    .connect(artifact.version_id.clone(), version.clone(), kind)
                    .map_err(HostError::Store)?;
            }
        }
        Ok(artifact.version_id)
    }
}

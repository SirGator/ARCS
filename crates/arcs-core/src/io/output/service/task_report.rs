//! Render a selected result while preserving its trust and ancestry.
use super::*;

pub const TASK_REPORT_SCHEMA: &str = "arcs.report.v1";

impl OutputService<'_> {
    /// The application binds the result to its task before calling this port.
    /// The resulting report can be previewed and approved for a destination.
    pub fn prepare_task_report(
        &mut self,
        runtime_id: &str,
        task_sequence: u64,
        result_version: &VersionId,
    ) -> Result<Artifact, OutputError> {
        self.render_task_report(runtime_id, task_sequence, result_version, None)
    }

    /// Publish only the fields that the plan's operator template exports.
    pub fn prepare_plan_report(
        &mut self,
        runtime_id: &str,
        task_sequence: u64,
        result_version: &VersionId,
        plan_version: &VersionId,
        fields: &[String],
    ) -> Result<Artifact, OutputError> {
        self.render_task_report(
            runtime_id,
            task_sequence,
            result_version,
            Some((plan_version, fields)),
        )
    }

    fn render_task_report(
        &mut self,
        runtime_id: &str,
        task_sequence: u64,
        result_version: &VersionId,
        plan: Option<(&VersionId, &[String])>,
    ) -> Result<Artifact, OutputError> {
        let result = self.load(result_version)?;
        let sequence = task_sequence.to_string();
        let mut parents = vec![result.version_id.0.clone()];
        let mut relations = vec![(result.version_id.clone(), relation_kinds::derived_from())];
        let (content, key) = if let Some((plan_version, fields)) = plan {
            let mut selected = serde_json::Map::new();
            for field in fields {
                let value = result
                    .payload
                    .get(field)
                    .ok_or_else(|| OutputError::MissingReportField(field.clone()))?;
                selected.insert(field.clone(), value.clone());
            }
            parents.push(plan_version.0.clone());
            relations.push((plan_version.clone(), relation_kinds::based_on()));
            (
                serde_json::Value::Object(selected),
                deterministic_invocation_id(
                    InvocationKind::Output,
                    &[
                        "plan-report-v1",
                        runtime_id,
                        &sequence,
                        &result.version_id.0,
                        &plan_version.0,
                        &serde_json::to_string(fields)?,
                    ],
                ),
            )
        } else {
            (
                result.payload,
                deterministic_invocation_id(
                    InvocationKind::Output,
                    &[
                        "task-report-v1",
                        runtime_id,
                        &sequence,
                        &result.version_id.0,
                    ],
                ),
            )
        };
        let payload = serde_json::json!({
            "title": format!("ARCS – Aufgabe {task_sequence}"),
            "body": serde_json::to_string_pretty(&content)?,
        });
        if serde_json::to_vec(&payload)?.len() > 65_536 {
            return Err(OutputError::InputTooLarge);
        }
        let stream = format!("task-output-report:{key}");
        let schema = self
            .schemas
            .get(&SchemaId(TASK_REPORT_SCHEMA.into()))
            .ok_or(OutputError::ContractMismatch)?
            .clone();
        let fingerprint = serde_json::to_string(&payload)?;
        let report = ArtifactFactory::new(self.clock, self.ids).create(ArtifactFactoryInput {
            schema,
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.task_output".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: stream.clone(),
            },
            trust: result.trust,
            stream_key: stream.clone(),
            subject: SubjectId(stream.clone()),
            tags: vec![],
            payload,
            provenance: Some(Provenance {
                parents,
                rules_applied: vec!["task_output.result_selected".into()],
                models_used: vec![],
                transform: Some("task_output.render_report".into()),
            }),
        })?;
        Ok(self
            .store
            .append_current_related_once(
                &report,
                self.schemas,
                Idempotency {
                    key: &stream,
                    input: &fingerprint,
                },
                &relations,
            )?
            .artifact)
    }
}

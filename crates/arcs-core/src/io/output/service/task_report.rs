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
        let result = self.load(result_version)?;
        let payload = serde_json::json!({
            "title": format!("ARCS – Aufgabe {task_sequence}"),
            "body": serde_json::to_string_pretty(&result.payload)?,
        });
        if serde_json::to_vec(&payload)?.len() > 65_536 {
            return Err(OutputError::InputTooLarge);
        }
        let key = deterministic_invocation_id(
            InvocationKind::Output,
            &[
                "task-report-v1",
                runtime_id,
                &task_sequence.to_string(),
                &result.version_id.0,
            ],
        );
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
                parents: vec![result.version_id.0.clone()],
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
                &[(result.version_id, relation_kinds::derived_from())],
            )?
            .artifact)
    }
}

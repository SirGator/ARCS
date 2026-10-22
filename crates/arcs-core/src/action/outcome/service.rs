use serde::Serialize;

use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::{OutcomeError, OutcomeEvaluator, OutcomeVerdict};

const OUTCOME_SCHEMA_ID: &str = "arcs.outcome.v1";

#[derive(Serialize)]
struct OutcomePayload {
    execution_result_version: String,
    verdict: OutcomeVerdict,
    detail: String,
}

/// Persistiert die Bewertung einer Ausführung ohne daraus selbst zu lernen.
pub struct OutcomeService<'a> {
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> OutcomeService<'a> {
    pub fn new(
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            schemas,
            store,
            ids,
            clock,
        }
    }

    pub fn evaluate(
        &mut self,
        execution_result: &VersionId,
        evaluator: &dyn OutcomeEvaluator,
    ) -> Result<Artifact, OutcomeError> {
        let execution_artifact = self
            .store
            .get(execution_result)?
            .ok_or_else(|| OutcomeError::MissingExecutionResult(execution_result.clone()))?;
        if execution_artifact.artifact_type != "execution_result" {
            return Err(OutcomeError::NotExecutionResult);
        }

        let result = evaluator.evaluate(&execution_artifact)?;
        let schema_id = SchemaId(OUTCOME_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| OutcomeError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let payload = serde_json::to_value(OutcomePayload {
            execution_result_version: execution_result.0.clone(),
            verdict: result.verdict,
            detail: result.detail,
        })?;
        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let outcome = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.outcome".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("outcome:{}", execution_result.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("outcome:{}", execution_result.0),
            subject: SubjectId(format!("outcome:{}", execution_result.0)),
            tags: vec![format!("execution_result:{}", execution_result.0)],
            payload,
            provenance: Some(Provenance {
                parents: vec![execution_result.0.clone()],
                rules_applied: vec!["outcome.evaluated".into()],
                models_used: vec![],
                transform: Some("outcome.report".into()),
            }),
        })?;

        self.store.append_related(
            &outcome,
            self.schemas,
            &[(execution_result.clone(), relation_kinds::evaluates())],
        )?;
        Ok(outcome)
    }
}

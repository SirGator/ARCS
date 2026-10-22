use serde_json::json;

use super::{OutcomeError, OutcomeEvaluator, OutcomeResult, OutcomeService, OutcomeVerdict};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

const NON_EXECUTION_SCHEMA: &str = r#"{
    "$id": "arcs.input.outcome_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["message"],
    "properties": {"message": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T20:00:00Z".into()
    }
}

struct TestIds(u64);

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.0;
        self.0 += 1;
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-{sequence}")),
            version_id: VersionId(format!("{artifact_type}-{sequence}-v1")),
        }
    }
}

struct FixedEvaluator {
    verdict: OutcomeVerdict,
    detail: &'static str,
}

impl OutcomeEvaluator for FixedEvaluator {
    fn evaluate(&self, _execution_result: &Artifact) -> Result<OutcomeResult, OutcomeError> {
        Ok(OutcomeResult {
            verdict: self.verdict,
            detail: self.detail.into(),
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(NON_EXECUTION_SCHEMA).unwrap();
    schemas
}

fn execution_result() -> Artifact {
    Artifact::new(
        "execution-result-1",
        "execution-result-1-v1",
        "execution_result",
        "arcs.execution_result.v1",
        "2026-08-08T19:59:00Z",
        Actor {
            actor_type: ActorType::Executor,
            id: "device.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "device-operation-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::External,
        },
        "execution:operation-1",
        json!({
            "action_version": "action-1-v1",
            "result": {"accepted": true}
        }),
    )
}

fn non_execution_artifact() -> Artifact {
    Artifact::new(
        "input-1",
        "input-1-v1",
        "input",
        "arcs.input.outcome_test.v1",
        "2026-08-08T19:59:00Z",
        Actor {
            actor_type: ActorType::Adapter,
            id: "input.test".into(),
        },
        Source {
            kind: SourceKind::Api,
            reference: "message-1".into(),
        },
        Trust {
            level: TrustLevel::Medium,
            source_class: SourceClass::External,
        },
        "input:message-1",
        json!({"message": "open door"}),
    )
}

fn evaluate(verdict: OutcomeVerdict, detail: &'static str) -> (Artifact, SqliteArtifactStore) {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let execution = execution_result();
    store.append(&execution, &schemas).unwrap();
    let mut ids = TestIds(1);
    let outcome = OutcomeService::new(&schemas, &store, &mut ids, &FixedClock)
        .evaluate(&execution.version_id, &FixedEvaluator { verdict, detail })
        .unwrap();
    (outcome, store)
}

#[test]
fn successful_outcome_is_persisted() {
    let (outcome, store) = evaluate(OutcomeVerdict::Success, "door sensor confirms open state");

    assert_eq!(outcome.artifact_type, "outcome");
    assert_eq!(outcome.created_by.actor_type, ActorType::System);
    assert_eq!(outcome.source.kind, SourceKind::Internal);
    assert_eq!(
        outcome.payload,
        json!({
            "execution_result_version": "execution-result-1-v1",
            "verdict": "success",
            "detail": "door sensor confirms open state"
        })
    );
    assert_eq!(store.get(&outcome.version_id).unwrap(), Some(outcome));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn failed_outcome_is_persisted() {
    let (outcome, store) = evaluate(OutcomeVerdict::Failure, "door sensor reports closed state");

    assert_eq!(outcome.payload["verdict"], json!("failure"));
    assert_eq!(store.get(&outcome.version_id).unwrap(), Some(outcome));
}

#[test]
fn unknown_outcome_is_preserved() {
    let (outcome, store) = evaluate(
        OutcomeVerdict::Unknown,
        "no door sensor evidence is available",
    );

    assert_eq!(outcome.payload["verdict"], json!("unknown"));
    assert_eq!(store.get(&outcome.version_id).unwrap(), Some(outcome));
}

#[test]
fn outcome_references_execution_result() {
    let (outcome, store) = evaluate(OutcomeVerdict::Success, "door sensor confirms open state");

    let relations = store.outgoing_relations(&outcome.version_id).unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].from, outcome.version_id);
    assert_eq!(relations[0].to, VersionId("execution-result-1-v1".into()));
    assert_eq!(relations[0].kind, relation_kinds::evaluates());
}

#[test]
fn non_execution_artifact_cannot_be_evaluated() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = non_execution_artifact();
    store.append(&input, &schemas).unwrap();
    let mut ids = TestIds(1);

    let result = OutcomeService::new(&schemas, &store, &mut ids, &FixedClock).evaluate(
        &input.version_id,
        &FixedEvaluator {
            verdict: OutcomeVerdict::Success,
            detail: "must not be called",
        },
    );

    assert!(matches!(result, Err(OutcomeError::NotExecutionResult)));
    assert_eq!(store.len().unwrap(), 1);
}

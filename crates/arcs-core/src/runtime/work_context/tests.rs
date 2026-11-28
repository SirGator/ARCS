use super::*;
use crate::core::security::SecurityProfile;
use crate::core::{Actor, ActorType, Artifact, Source, SourceClass, SourceKind, Trust, TrustLevel};
use serde_json::json;

struct Time;
impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        "2026-11-27T09:00:00Z".into()
    }
}

fn schemas() -> SchemaRegistry {
    SchemaRegistry::with_bundled_schemas().unwrap()
}

fn store() -> SqliteArtifactStore {
    SqliteArtifactStore::in_memory().unwrap()
}

/// Legt ein Inputartefakt mit der gewünschten Schutzstufe an.
fn seed(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    id: &str,
    security: SecurityProfile,
) {
    let artifact = Artifact {
        security,
        ..Artifact::new(
            id,
            format!("{id}-v1"),
            "input",
            "arcs.input.v1",
            "2026-11-27T09:00:00Z",
            Actor {
                actor_type: ActorType::Human,
                id: "user".into(),
            },
            Source {
                kind: SourceKind::Chat,
                reference: "test".into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::Human,
            },
            "test",
            json!({"raw_text":"test"}),
        )
    };
    store.append(&artifact, schemas).unwrap();
}

fn draft() -> WorkContextDraft {
    WorkContextDraft {
        runtime_id: "host-a".into(),
        task_sequence: 7,
        route_id: "route-a".into(),
        goal_id: Some("goal-1".into()),
        plan_revision: Some("plan-v3".into()),
        authority: AuthorityScope {
            role_id: "operator".into(),
            capabilities: vec!["reason.propose".into()],
            permissions: vec!["memory.read".into()],
        },
        memory: MemoryView {
            schema_ids: vec!["arcs.input.v1".into()],
            max_classification: Classification::Internal,
        },
        priority: 1,
        budget: WorkBudget {
            max_model_calls: 4,
            max_tokens: 8_000,
            max_cost_micros: 5_000,
            max_wall_clock_ms: 60_000,
        },
        artifact_versions: Vec::new(),
    }
}

fn service<'a>(
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
) -> WorkContextService<'a> {
    WorkContextService::new(store, schemas, &Time)
}

#[test]
fn open_creates_a_pending_context_with_a_current_pointer() {
    let store = store();
    let schemas = schemas();
    let context = service(&store, &schemas).open(&draft()).unwrap();

    assert_eq!(context.state, WorkContextState::Pending);
    assert_eq!(context.version, 1);
    assert_eq!(context.task_sequence, 7);
    assert_eq!(context.route_id, "route-a");
    assert_eq!(context.goal_id.as_deref(), Some("goal-1"));
    assert_eq!(context.plan_revision.as_deref(), Some("plan-v3"));
    assert_eq!(context.priority, 1);
    assert_eq!(context.budget.max_model_calls, 4);
    assert!(context.waiting_for.is_empty());

    let current = service(&store, &schemas)
        .current("host-a", 7)
        .unwrap()
        .expect("current context");
    assert_eq!(current.version_id, context.version_id);
}

#[test]
fn open_is_idempotent_for_the_same_task() {
    let store = store();
    let schemas = schemas();
    let first = service(&store, &schemas).open(&draft()).unwrap();
    let second = service(&store, &schemas).open(&draft()).unwrap();

    assert_eq!(first.version_id, second.version_id);
    assert_eq!(store.len().unwrap(), 1);
}

#[test]
fn open_rejects_a_different_route_for_the_same_task() {
    let store = store();
    let schemas = schemas();
    service(&store, &schemas).open(&draft()).unwrap();
    let mut other = draft();
    other.route_id = "route-b".into();

    let error = service(&store, &schemas).open(&other).unwrap_err();
    assert!(matches!(error, WorkContextError::AlreadyOpen(_)));
}

#[test]
fn advance_records_state_operations_and_results() {
    let store = store();
    let schemas = schemas();
    seed(&store, &schemas, "input", SecurityProfile::default());
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();

    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    assert_eq!(context.state, WorkContextState::Running);
    assert_eq!(context.version, 2);

    context = service
        .advance(
            &context,
            &[
                ContextChange::Artifact {
                    version: VersionId("input-v1".into()),
                },
                ContextChange::Operation {
                    operation: ContextOperation {
                        operation_id: "op-1".into(),
                        kind: "reasoning".into(),
                        state: OperationState::Running,
                        version_id: None,
                    },
                },
            ],
        )
        .unwrap();
    assert_eq!(
        context.artifact_versions,
        vec![VersionId("input-v1".into())]
    );
    assert_eq!(context.operations.len(), 1);

    context = service
        .bind_operation(
            &context,
            ContextOperation {
                operation_id: "op-1".into(),
                kind: "reasoning".into(),
                state: OperationState::Succeeded,
                version_id: None,
            },
            Some(&VersionId("result-v1".into())),
        )
        .unwrap();
    assert_eq!(context.operations[0].state, OperationState::Succeeded);
    assert_eq!(
        context.operations[0].version_id,
        Some(VersionId("result-v1".into()))
    );

    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::ReadyForReview,
                waiting_for: Vec::new(),
                reason: "reasoning done".into(),
            }],
        )
        .unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Completed,
                waiting_for: Vec::new(),
                reason: "approved".into(),
            }],
        )
        .unwrap();
    assert_eq!(context.state, WorkContextState::Completed);
    assert!(context.state.is_terminal());

    let history = service.history("host-a", 7).unwrap();
    assert_eq!(history.len(), 6);
    assert_eq!(history.last().unwrap().state, WorkContextState::Completed);
}

#[test]
fn advance_rejects_an_illegal_transition() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let context = service.open(&draft()).unwrap();

    let error = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Completed,
                waiting_for: Vec::new(),
                reason: "skip".into(),
            }],
        )
        .unwrap_err();
    assert!(matches!(error, WorkContextError::Transition(_)));
    // Der abgelehnte Übergang hinterlässt keine Spur.
    assert_eq!(service.current("host-a", 7).unwrap().unwrap().version, 1);
}

#[test]
fn advance_rejects_completion_with_unfinished_operations() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::Operation {
                operation: ContextOperation {
                    operation_id: "op-1".into(),
                    kind: "execution".into(),
                    state: OperationState::Running,
                    version_id: None,
                },
            }],
        )
        .unwrap();

    let error = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Completed,
                waiting_for: Vec::new(),
                reason: "premature".into(),
            }],
        )
        .unwrap_err();
    assert!(matches!(error, WorkContextError::Transition(_)));
}

#[test]
fn advance_rejects_a_waiting_state_without_condition() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();

    let error = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Waiting,
                waiting_for: Vec::new(),
                reason: "waiting".into(),
            }],
        )
        .unwrap_err();
    assert!(matches!(error, WorkContextError::Invalid(_)));

    let error = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: vec![WaitCondition {
                    kind: WaitKind::Resource,
                    reference: "camera".into(),
                }],
                reason: "running with wait".into(),
            }],
        )
        .unwrap_err();
    assert!(matches!(error, WorkContextError::Invalid(_)));
}

#[test]
fn waiting_state_keeps_its_conditions() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Waiting,
                waiting_for: vec![WaitCondition {
                    kind: WaitKind::Resource,
                    reference: "camera".into(),
                }],
                reason: "resource busy".into(),
            }],
        )
        .unwrap();
    assert_eq!(context.state, WorkContextState::Waiting);
    assert_eq!(context.waiting_for.len(), 1);
    assert_eq!(context.waiting_for[0].kind, WaitKind::Resource);
}

#[test]
fn a_waiting_context_can_finish_after_the_wait_ends() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Waiting,
                waiting_for: vec![WaitCondition {
                    kind: WaitKind::Adapter,
                    reference: "example.solver".into(),
                }],
                reason: "reasoner busy".into(),
            }],
        )
        .unwrap();

    // Der Wartezustand darf nach dem Ende der Wartebedingung abgeschlossen
    // werden; die Bedingung selbst wird dabei aufgelöst.
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::ReadyForReview,
                waiting_for: Vec::new(),
                reason: "reasoner available".into(),
            }],
        )
        .unwrap();
    assert_eq!(context.state, WorkContextState::ReadyForReview);
    assert!(context.waiting_for.is_empty());
}

#[test]
fn open_rejects_an_unknown_artifact_reference() {
    let store = store();
    let schemas = schemas();
    let mut draft = draft();
    draft.artifact_versions = vec![VersionId("missing-v1".into())];

    let error = service(&store, &schemas).open(&draft).unwrap_err();
    assert!(matches!(error, WorkContextError::UnknownArtifact(_)));
    assert_eq!(store.len().unwrap(), 0);
}

#[test]
fn open_rejects_an_unregistered_memory_schema() {
    let store = store();
    let schemas = schemas();
    let mut draft = draft();
    draft.memory.schema_ids = vec!["arcs.unknown.v1".into()];

    let error = service(&store, &schemas).open(&draft).unwrap_err();
    assert!(matches!(error, WorkContextError::Invalid(_)));
}

#[test]
fn open_rejects_an_oversized_budget() {
    let store = store();
    let schemas = schemas();
    let mut draft = draft();
    draft.budget.max_wall_clock_ms = MAX_BUDGET_WALL_CLOCK_MS + 1;

    let error = service(&store, &schemas).open(&draft).unwrap_err();
    assert!(matches!(error, WorkContextError::Invalid(_)));
}

#[test]
fn a_context_inherits_the_classification_of_its_references() {
    let store = store();
    let schemas = schemas();
    seed(
        &store,
        &schemas,
        "secret",
        SecurityProfile::declared(Classification::Secret),
    );
    let mut draft = draft();
    draft.artifact_versions = vec![VersionId("secret-v1".into())];

    let context = service(&store, &schemas).open(&draft).unwrap();
    assert_eq!(context.classification, Classification::Secret);
}

#[test]
fn a_context_never_lowers_its_classification() {
    let store = store();
    let schemas = schemas();
    seed(
        &store,
        &schemas,
        "secret",
        SecurityProfile::declared(Classification::Secret),
    );
    let service = service(&store, &schemas);
    let mut draft = draft();
    draft.artifact_versions = vec![VersionId("secret-v1".into())];
    let mut context = service.open(&draft).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    assert_eq!(context.classification, Classification::Secret);
}

#[test]
fn a_stale_context_is_rejected_instead_of_overwriting_a_newer_one() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let context = service.open(&draft()).unwrap();
    let running = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();

    // Ein veralteter Stand schreibt gegen die bereits vorhandene Version 2.
    let error = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Failed,
                waiting_for: Vec::new(),
                reason: "stale".into(),
            }],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        WorkContextError::Store(StoreError::VersionConflict { .. })
    ));

    // Der aktuelle Stand bleibt unberührt und kann fortgeschrieben werden.
    let current = service.current("host-a", 7).unwrap().unwrap();
    assert_eq!(current.version_id, running.version_id);
}

#[test]
fn a_restart_continues_the_persisted_context() {
    let directory = std::env::temp_dir().join(format!(
        "arcs-work-context-{}-{}",
        std::process::id(),
        "restart"
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("core.sqlite");
    let schemas = schemas();

    {
        let store = SqliteArtifactStore::open(&path).unwrap();
        let service = service(&store, &schemas);
        let context = service.open(&draft()).unwrap();
        service
            .advance(
                &context,
                &[ContextChange::State {
                    state: WorkContextState::Running,
                    waiting_for: Vec::new(),
                    reason: "dispatch".into(),
                }],
            )
            .unwrap();
    }

    {
        let store = SqliteArtifactStore::open(&path).unwrap();
        let service = service(&store, &schemas);
        let resumed = service.current("host-a", 7).unwrap().unwrap();
        assert_eq!(resumed.state, WorkContextState::Running);
        assert_eq!(resumed.version, 2);
        let continued = service
            .advance(
                &resumed,
                &[ContextChange::State {
                    state: WorkContextState::Failed,
                    waiting_for: Vec::new(),
                    reason: "resume".into(),
                }],
            )
            .unwrap();
        assert_eq!(continued.version, 3);
    }

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_foreign_artifact_is_not_read_as_a_work_context() {
    let store = store();
    let schemas = schemas();
    let artifact = Artifact::new(
        "work-context:0123456789abcdef",
        "work-context:0123456789abcdef:v1",
        "work_context",
        SCHEMA,
        "2026-11-27T09:00:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "model".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "model".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "work-context:host-a:7",
        json!({
            "context_id": "work-context:0123456789abcdef",
            "runtime_id": "host-a",
            "task_sequence": 7,
            "route_id": "route-a",
            "state": "running",
            "artifact_versions": [],
            "authority": {"role_id": "operator", "capabilities": [], "permissions": []},
            "memory": {"schema_ids": [], "max_classification": "internal"},
            "priority": 0,
            "budget": {"max_model_calls": 0, "max_tokens": 0, "max_cost_micros": 0, "max_wall_clock_ms": 0},
            "operations": []
        }),
    );
    store.append(&artifact, &schemas).unwrap();

    let error = service(&store, &schemas)
        .current("host-a", 7)
        .and_then(|context| context.ok_or_else(|| invalid("missing context")))
        .unwrap_err();
    assert!(matches!(error, WorkContextError::Invalid(_)));
}

#[test]
fn rebinding_the_same_result_is_idempotent() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();

    let operation = ContextOperation {
        operation_id: "op-1".into(),
        kind: "reasoning".into(),
        state: OperationState::Succeeded,
        version_id: None,
    };
    context = service
        .bind_operation(
            &context,
            operation.clone(),
            Some(&VersionId("result-v1".into())),
        )
        .unwrap();
    let version_after_first = context.version;

    // Ein erneutes Binden desselben Ergebnisses darf den Stand nicht
    // fortschreiben und keinen Fehler erzeugen.
    context = service
        .bind_operation(
            &context,
            operation.clone(),
            Some(&VersionId("result-v1".into())),
        )
        .unwrap();
    assert_eq!(context.version, version_after_first);
    assert_eq!(context.operations[0].state, OperationState::Succeeded);
    assert_eq!(
        context.operations[0].version_id,
        Some(VersionId("result-v1".into()))
    );
}

#[test]
fn rebinding_a_different_result_is_rejected() {
    let store = store();
    let schemas = schemas();
    let service = service(&store, &schemas);
    let mut context = service.open(&draft()).unwrap();
    context = service
        .advance(
            &context,
            &[ContextChange::State {
                state: WorkContextState::Running,
                waiting_for: Vec::new(),
                reason: "dispatch".into(),
            }],
        )
        .unwrap();
    context = service
        .bind_operation(
            &context,
            ContextOperation {
                operation_id: "op-1".into(),
                kind: "reasoning".into(),
                state: OperationState::Succeeded,
                version_id: None,
            },
            Some(&VersionId("result-v1".into())),
        )
        .unwrap();

    let error = service
        .bind_operation(
            &context,
            ContextOperation {
                operation_id: "op-1".into(),
                kind: "reasoning".into(),
                state: OperationState::Succeeded,
                version_id: None,
            },
            Some(&VersionId("result-v2".into())),
        )
        .unwrap_err();
    assert!(
        matches!(error, WorkContextError::Transition(_)),
        "{error:?}"
    );
}

use super::*;
use serde_json::json;
struct Fixture {
    store: SqliteArtifactStore,
    schemas: SchemaRegistry,
    registry: AdapterRegistry,
    ids: SequenceIdGenerator,
    definition: ReconnectDefinition,
}
impl Fixture {
    fn new() -> Self {
        let store = SqliteArtifactStore::in_memory().unwrap();
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        schemas.register_json(r#"{"$id":"arcs.reconnect_candidate.test.v1","type":"object","properties":{},"additionalProperties":false}"#).unwrap();
        let manifest=serde_json::from_value(json!({"protocol_version":1,"adapter_id":"peer","adapter_version":"1","capabilities":[{"id":"propose","contract":{"kind":"reason","emits":["arcs.reconnect_candidate.test.v1"]},"required_permissions":[]}]})).unwrap();
        let grant=serde_json::from_value(json!({"adapter_id":"peer","producer_class":"model","enabled_capabilities":["propose"],"granted_permissions":[],"assigned_trust":"low","ingress_source_kind":null,"max_payload_bytes":4096,"max_external_reference_bytes":256,"reasoning_limits":null,"reasoning_limits_v2":{"methods":["learned"],"resources":{"max_context_items":4,"max_context_bytes":16384,"max_output_bytes":4096,"max_candidates":4,"max_runtime_ms":1000,"max_iterations":null,"max_output_tokens":null}}})).unwrap();
        let definition = ReconnectDefinition {
            capabilities_verified: false,
            node_id: "node".into(),
            manifest,
            grant,
            policy: ReconnectPolicy {
                max_attempts: 2,
                delay_ms: 100,
                handshake_timeout_ms: 500,
            },
        };
        let mut registry = AdapterRegistry::new();
        registry
            .register(
                definition.manifest.clone(),
                definition.grant.clone(),
                &schemas,
                &store,
            )
            .unwrap();
        let ids = store.durable_id_generator("reconnect-test").unwrap();
        Self {
            store,
            schemas,
            registry,
            ids,
            definition,
        }
    }
    fn service(&mut self) -> ReconnectService<'_> {
        ReconnectService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
    }
    fn connected(&mut self) -> ReconnectReceipt {
        let definition = self.definition.clone();
        self.service().observe_connected(&definition).unwrap()
    }
}
#[test]
fn bounded_episodes_preserve_failure_history_and_success_does_not_reset_task_records() {
    let mut f = Fixture::new();
    let initial = f.connected();
    let def = initial.record.definition_version_id.clone();
    assert_eq!(f.connected().version_id, initial.version_id);
    let waiting = f.service().note_loss(&def, 1000).unwrap();
    assert_eq!(waiting.record.not_before_unix_ms, Some(1100));
    assert_eq!(
        f.service().note_loss(&def, 5000).unwrap().version_id,
        waiting.version_id
    );
    assert!(f.service().claim(&def, &waiting.version_id, 1099).is_err());
    let first = f.service().claim(&def, &waiting.version_id, 1100).unwrap();
    assert_eq!(first.record.attempt, 1);
    let waiting2 = f
        .service()
        .finish(
            &def,
            &first.version_id,
            Some(ConnectionFailure::Transport),
            1200,
        )
        .unwrap();
    let second = f.service().claim(&def, &waiting2.version_id, 1300).unwrap();
    let healthy = f
        .service()
        .finish(&def, &second.version_id, None, 1400)
        .unwrap();
    assert_eq!(healthy.record.state, ConnectionState::Healthy);
    assert_eq!(healthy.record.attempt, 2);
    let next = f.service().note_loss(&def, 1500).unwrap();
    assert_eq!(next.record.generation, 2);
    assert_eq!(next.record.attempt, 0);
    assert!(f.store.get(&waiting2.version_id).unwrap().is_some());
}
#[test]
fn exhaustion_contract_rejection_and_stale_completions_are_explicit() {
    for contract in [true, false] {
        let mut f = Fixture::new();
        f.definition.policy.max_attempts = 1;
        let initial = f.connected();
        let def = initial.record.definition_version_id;
        let waiting = f.service().note_loss(&def, 1000).unwrap();
        let claim = f.service().claim(&def, &waiting.version_id, 1100).unwrap();
        let failure = if contract {
            ConnectionFailure::Contract
        } else {
            ConnectionFailure::Timeout
        };
        let final_state = f
            .service()
            .finish(&def, &claim.version_id, Some(failure), 1200)
            .unwrap();
        assert_eq!(
            final_state.record.state,
            if contract {
                ConnectionState::Rejected
            } else {
                ConnectionState::Exhausted
            }
        );
        assert!(
            f.service()
                .finish(&def, &claim.version_id, None, 1200)
                .is_err()
        );
        assert!(
            f.service()
                .claim(&def, &final_state.version_id, 9999)
                .is_err()
        );
        assert_eq!(
            f.service().note_loss(&def, 9999).unwrap().version_id,
            final_state.version_id
        );
    }
}
#[test]
fn current_grants_policy_limits_and_untrusted_state_cannot_be_bypassed() {
    let mut f = Fixture::new();
    let mut invalid = f.definition.clone();
    invalid.policy.max_attempts = 0;
    assert!(f.service().observe_connected(&invalid).is_err());
    invalid = f.definition.clone();
    invalid.grant.max_payload_bytes += 1;
    assert!(f.service().observe_connected(&invalid).is_err());
    let state = f.connected();
    let def = state.record.definition_version_id.clone();
    let mut forged = f.store.get(&state.version_id).unwrap().unwrap();
    forged.created_by.actor_type = ActorType::Model;
    forged.trust.level = TrustLevel::Low;
    let mut other = Fixture::new();
    let definition = f.store.get(&def).unwrap().unwrap();
    other.store.append(&definition, &other.schemas).unwrap();
    other
        .store
        .append_current_compared(
            &[forged],
            &other.schemas,
            &[(records::subject(&def), SchemaId(STATE_SCHEMA.into()), None)],
        )
        .unwrap();
    assert!(other.service().current(&def).is_err());
}

#[test]
fn independent_connections_claim_one_attempt_and_preserve_the_due_time_after_reopen() {
    let mut f = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "arcs-reconnect-race-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.ids = f.store.durable_id_generator("race").unwrap();
    let initial = f.connected();
    let definition = initial.record.definition_version_id;
    let waiting = f.service().note_loss(&definition, 1000).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|index| {
            let path = path.clone();
            let registry = f.registry.clone();
            let schemas = f.schemas.clone();
            let definition = definition.clone();
            let expected = waiting.version_id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteArtifactStore::open(path).unwrap();
                let mut ids = store
                    .durable_id_generator(&format!("writer{index}"))
                    .unwrap();
                barrier.wait();
                ReconnectService {
                    registry: &registry,
                    schemas: &schemas,
                    store: &store,
                    ids: &mut ids,
                    clock: &SystemClock,
                }
                .claim(&definition, &expected, 1100)
            })
        })
        .collect::<Vec<_>>();
    let outcomes = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(ReconnectError::Conflict)))
            .count(),
        1
    );
    f.store = SqliteArtifactStore::open(&path).unwrap();
    let current = f.service().current(&definition).unwrap();
    assert_eq!(current.record.attempt, 1);
    let failed = f
        .service()
        .finish(
            &definition,
            &current.version_id,
            Some(ConnectionFailure::Timeout),
            1200,
        )
        .unwrap();
    drop(f);
    let mut f = Fixture::new();
    f.store = SqliteArtifactStore::open(&path).unwrap();
    assert_eq!(
        f.service().current(&definition).unwrap().version_id,
        failed.version_id
    );
    assert_eq!(failed.record.not_before_unix_ms, Some(1300));
    drop(f);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn a_failed_optimistic_commit_rolls_back_state_provenance_and_attempt_consumption() {
    let mut f = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "arcs-reconnect-rollback-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.ids = f.store.durable_id_generator("rollback").unwrap();
    let initial = f.connected();
    let definition = initial.record.definition_version_id;
    let waiting = f.service().note_loss(&definition, 1000).unwrap();
    let count = f.store.len().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER interrupted_claim BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.adapter_reconnect_state.v1' AND json_extract(NEW.artifact_json,'$.payload.state')='connecting' BEGIN SELECT RAISE(ABORT,'lost reconnect claim'); END;").unwrap();
    assert!(
        f.service()
            .claim(&definition, &waiting.version_id, 1100)
            .is_err()
    );
    assert_eq!(f.store.len().unwrap(), count);
    assert_eq!(
        f.service().current(&definition).unwrap().version_id,
        waiting.version_id
    );
    assert_eq!(f.service().current(&definition).unwrap().record.attempt, 0);
    db.execute_batch("DROP TRIGGER interrupted_claim").unwrap();
    drop(db);
    assert_eq!(
        f.service()
            .claim(&definition, &waiting.version_id, 1100)
            .unwrap()
            .record
            .attempt,
        1
    );
    drop(f);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn generic_current_updates_bind_each_expected_schema_and_reject_stale_heads() {
    let mut f = Fixture::new();
    let initial = f.connected();
    let definition = initial.record.definition_version_id;
    let old = f.store.get(&initial.version_id).unwrap().unwrap();
    let mut next = old.clone();
    next.version += 1;
    next.version_id = VersionId(format!("{}:v{}", next.artifact_id.0, next.version));
    next.provenance
        .as_mut()
        .unwrap()
        .parents
        .push(old.version_id.0.clone());
    let expected = (
        old.subject.clone().unwrap(),
        old.schema_id.clone(),
        Some(old.version_id.clone()),
    );
    assert!(
        f.store
            .append_current_compared(
                std::slice::from_ref(&next),
                &f.schemas,
                &[(
                    expected.0.clone(),
                    SchemaId("arcs.input.v1".into()),
                    expected.2.clone()
                )]
            )
            .is_err()
    );
    assert_eq!(
        f.service().current(&definition).unwrap().version_id,
        initial.version_id
    );
    f.store
        .append_current_compared(
            std::slice::from_ref(&next),
            &f.schemas,
            std::slice::from_ref(&expected),
        )
        .unwrap();
    assert!(
        f.store
            .append_current_compared(&[next], &f.schemas, &[expected])
            .is_err()
    );
    assert_eq!(
        f.service().current(&definition).unwrap().record.state,
        ConnectionState::Healthy
    );
}

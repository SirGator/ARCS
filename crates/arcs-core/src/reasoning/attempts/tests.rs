use super::*;
use crate::reasoning::{
    general::{GeneralReasoningAdapter, ReasoningPreparation},
    test_support::Fixture,
};
use crate::runtime::{InvocationService, InvocationStatus};
use crate::store::{ArtifactRelations, SqliteArtifactStore};

fn artifacts(f: &Fixture) -> Vec<Artifact> {
    f.store
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|event| event.artifact)
        .filter(|artifact| artifact.schema_id.0 == SCHEMA)
        .collect()
}
fn query(limit: usize) -> AttemptQuery {
    AttemptQuery {
        capabilities: vec![Fixture::request().capability],
        after_sequence: 0,
        limit,
    }
}
fn complete(f: &mut Fixture) {
    let prepared = f.dispatch();
    let response = f.peer.propose_v2(prepared.invocation()).unwrap();
    f.service().finish(prepared, Ok(response)).unwrap();
}

#[test]
fn valid_responses_have_one_atomic_linked_record_and_cache_replay_adds_none() {
    let mut f = Fixture::new();
    complete(&mut f);
    let records = artifacts(&f);
    assert_eq!(records.len(), 1);
    let record = read(&f.store, &f.schemas, &records[0]).unwrap();
    assert!(record.host_completion_latency_ms.is_some());
    assert!(matches!(
        record.outcome,
        AttemptOutcome::Response {
            status: ReasoningStatus::Solved,
            candidate_count: 1,
            ..
        }
    ));
    let page = history(&f.registry, &f.schemas, &f.store, &query(10)).unwrap();
    assert_eq!(page.page_counts.valid_responses, 1);
    assert_eq!(page.page_counts.solved_candidate_responses, 1);
    assert!(!page.candidate_quality_verified);
    assert!(
        !serde_json::to_string(&page).unwrap().contains("continue"),
        "input text leaked through history"
    );
    let count = f.store.len().unwrap();
    assert!(matches!(f.prepare(), ReasoningPreparation::Complete(_)));
    assert_eq!(f.store.len().unwrap(), count);
    assert_eq!(artifacts(&f).len(), 1);
    let edges = ArtifactRelations::new(&f.store)
        .outgoing(&records[0].version_id)
        .unwrap();
    assert!(
        edges
            .iter()
            .any(|edge| edge.to == record.dispatch_version_id)
    );
    assert!(
        edges.iter().any(|edge| edge.to.0 == "input-v1"),
        "attempt experience is not reachable from the original context"
    );
}

#[test]
fn a_retry_keeps_each_actual_dispatch_outcome_and_never_relabels_the_old_failure() {
    let mut f = Fixture::new();
    let prepared = f.dispatch();
    let id = prepared.invocation().invocation_id.clone();
    assert!(
        f.service()
            .finish(
                prepared,
                Err(ReasoningError::AdapterCall(AdapterCallError::Unavailable(
                    "SECRET external error".into()
                )))
            )
            .is_err()
    );
    let first = artifacts(&f).remove(0);
    let failed = read(&f.store, &f.schemas, &first).unwrap();
    assert!(
        ArtifactRelations::new(&f.store)
            .outgoing(&first.version_id)
            .unwrap()
            .iter()
            .any(|edge| edge.to.0 == "input-v1")
    );
    let mut altered = first.clone();
    altered.payload["outcome"]["failure"] = serde_json::json!("adapter_timeout");
    assert!(
        read(&f.store, &f.schemas, &altered).is_err(),
        "caller-provided metadata replaced immutable evidence"
    );
    assert!(matches!(
        failed.outcome,
        AttemptOutcome::Failed {
            failure: ReasoningFailureKind::AdapterUnavailable,
            retryable: true
        }
    ));
    assert_eq!(
        InvocationService::new(&f.store, &f.schemas, &SystemClock)
            .lookup(&id)
            .unwrap()
            .unwrap()
            .status,
        InvocationStatus::Failed
    );
    complete(&mut f);
    let records = artifacts(&f);
    assert_eq!(records.len(), 2);
    let old = read(&f.store, &f.schemas, &first).unwrap();
    assert_eq!(old, failed);
    let new = read(&f.store, &f.schemas, &records[1]).unwrap();
    assert_ne!(old.dispatch_version_id, new.dispatch_version_id);
    assert_eq!(old.invocation_id, new.invocation_id);
    let mut q = query(1);
    let first_page = history(&f.registry, &f.schemas, &f.store, &q).unwrap();
    assert!(first_page.has_more);
    assert_eq!(first_page.page_counts.failures, 1);
    assert_eq!(first_page.page_counts.retryable_failures, 1);
    assert!(
        !serde_json::to_string(&first_page)
            .unwrap()
            .contains("SECRET")
    );
    q.after_sequence = first_page.next_sequence;
    let second = history(&f.registry, &f.schemas, &f.store, &q).unwrap();
    assert!(!second.has_more);
    assert_eq!(second.attempts.len(), 1);
    assert_eq!(second.page_counts.valid_responses, 1);
}

#[test]
fn no_solution_is_a_valid_response_while_invalid_payload_is_an_invalid_response_failure() {
    for valid in [true, false] {
        let mut f = Fixture::new();
        let prepared = f.dispatch();
        let mut response = f.peer.propose_v2(prepared.invocation()).unwrap();
        if valid {
            response.status = ReasoningStatus::NoSolution;
            response.candidates.clear();
        } else {
            response.candidates[0].payload = serde_json::json!({"wrong":"untrusted"});
        }
        let result = f.service().finish(prepared, Ok(response));
        assert_eq!(result.is_ok(), valid);
        let record = read(&f.store, &f.schemas, &artifacts(&f)[0]).unwrap();
        if valid {
            assert!(matches!(
                record.outcome,
                AttemptOutcome::Response {
                    status: ReasoningStatus::NoSolution,
                    candidate_count: 0,
                    ..
                }
            ));
        } else {
            assert!(matches!(
                record.outcome,
                AttemptOutcome::Failed {
                    failure: ReasoningFailureKind::InvalidResponse,
                    retryable: false
                }
            ));
        }
    }
}

#[test]
fn forged_or_unlinked_records_are_not_used_as_failure_evidence_or_page_counts() {
    let mut f = Fixture::new();
    complete(&mut f);
    let original = artifacts(&f).remove(0);
    let mut forged = original.clone();
    forged.artifact_id.0 = "fake".into();
    forged.version_id.0 = "fake-v1".into();
    forged.created_by.actor_type = ActorType::Model;
    forged.trust.level = TrustLevel::Low;
    f.store.append(&forged, &f.schemas).unwrap();
    assert!(read(&f.store, &f.schemas, &forged).is_err());
    let page = history(&f.registry, &f.schemas, &f.store, &query(10)).unwrap();
    assert_eq!(page.attempts.len(), 1);
    assert_eq!(page.skipped_untrusted, 1);
    let mut tampered = original.clone();
    tampered.payload["outcome"] =
        serde_json::json!({"kind":"failed","failure":"adapter_timeout","retryable":true});
    assert!(read(&f.store, &f.schemas, &tampered).is_err());
    let mut orphan = original;
    orphan.version_id.0 = "unlinked-v1".into();
    assert!(read(&f.store, &f.schemas, &orphan).is_err());
}

#[test]
fn legacy_or_unknown_invocations_are_not_fabricated_into_observed_outcomes() {
    let mut f = Fixture::new();
    let old = f.dispatch();
    let old_response = f.peer.propose_v2(old.invocation()).unwrap();
    let current = f.dispatch();
    assert!(f.service().finish(old, Ok(old_response.clone())).is_err());
    assert!(artifacts(&f).is_empty());
    f.service().finish(current, Ok(old_response)).unwrap();
    assert_eq!(artifacts(&f).len(), 1);
    let mut q = query(10);
    q.capabilities.clear();
    assert!(history(&f.registry, &f.schemas, &f.store, &q).is_err());
    let mut q = query(10);
    q.capabilities[0].adapter_id.0 = "not-enabled".into();
    assert!(history(&f.registry, &f.schemas, &f.store, &q).is_err());
}

#[test]
fn attempt_records_and_graph_evidence_survive_reopening_sqlite() {
    let mut f = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "arcs-attempt-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let input = f.store.get(&VersionId("input-v1".into())).unwrap().unwrap();
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.store.append(&input, &f.schemas).unwrap();
    f.ids = f.store.durable_id_generator("attempt-test").unwrap();
    complete(&mut f);
    let expected = artifacts(&f).remove(0);
    f.store = SqliteArtifactStore::in_memory().unwrap();
    f.store = SqliteArtifactStore::open(&path).unwrap();
    let page = history(&f.registry, &f.schemas, &f.store, &query(10)).unwrap();
    assert_eq!(page.attempts[0].version_id, expected.version_id);
    assert!(matches!(f.prepare(), ReasoningPreparation::Complete(_)));
    assert_eq!(artifacts(&f).len(), 1);
    drop(f);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_failed_publication_rolls_back_attempt_result_and_final_state_together() {
    for success in [true, false] {
        let mut f = Fixture::new();
        let path = std::env::temp_dir().join(format!(
            "arcs-attempt-rollback-{}-{}-{success}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let input = f.store.get(&VersionId("input-v1".into())).unwrap().unwrap();
        f.store = SqliteArtifactStore::open(&path).unwrap();
        f.store.append(&input, &f.schemas).unwrap();
        f.ids = f.store.durable_id_generator("rollback").unwrap();
        let prepared = f.dispatch();
        let id = prepared.invocation().invocation_id.clone();
        let response = f.peer.propose_v2(prepared.invocation()).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TRIGGER fail_final_state BEFORE INSERT ON artifact_versions WHEN NEW.schema_id='arcs.invocation.v1' AND json_extract(NEW.artifact_json,'$.payload.status') IN ('succeeded','failed') BEGIN SELECT RAISE(ABORT,'interrupted evidence commit'); END;").unwrap();
        let response = if success {
            Ok(response)
        } else {
            Err(ReasoningError::AdapterCall(AdapterCallError::Timeout))
        };
        assert!(f.service().finish(prepared, response).is_err());
        assert!(artifacts(&f).is_empty());
        assert_eq!(
            InvocationService::new(&f.store, &f.schemas, &SystemClock)
                .lookup(&id)
                .unwrap()
                .unwrap()
                .status,
            InvocationStatus::Dispatched
        );
        let results: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM artifact_versions WHERE schema_id='arcs.reasoning_result.v2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(results, 0);
        db.execute_batch("DROP TRIGGER fail_final_state").unwrap();
        drop(db);
        complete(&mut f);
        assert_eq!(artifacts(&f).len(), 1);
        drop(f);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn scoped_pages_respect_the_snapshot_and_advance_over_large_rows() {
    let f = Fixture::new();
    let original = f.store.get(&VersionId("input-v1".into())).unwrap().unwrap();
    for (id, size) in [("small", 5), ("large", 5000)] {
        let mut artifact = original.clone();
        artifact.artifact_id.0 = id.into();
        artifact.version_id.0 = format!("{id}-v1");
        artifact.stream_key = id.into();
        artifact.subject = Some(SubjectId("scope:a".into()));
        artifact.payload = serde_json::json!({"raw_text":"x".repeat(size)});
        f.store.append(&artifact, &f.schemas).unwrap();
    }
    let through = f.store.latest_sequence().unwrap();
    let mut late = original;
    late.artifact_id.0 = "late".into();
    late.version_id.0 = "late-v1".into();
    late.stream_key = "late".into();
    late.subject = Some(SubjectId("scope:a".into()));
    f.store.append(&late, &f.schemas).unwrap();
    let rows = f
        .store
        .schema_subject_page_after(
            &SchemaId("arcs.input.v1".into()),
            &SubjectId("scope:a".into()),
            0,
            through,
            10,
            2048,
        )
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].artifact.is_some());
    assert!(rows[1].artifact.is_none());
    assert_eq!(rows[1].sequence, through);
    assert!(
        f.store
            .schema_subject_page_after(
                &SchemaId("arcs.input.v1".into()),
                &SubjectId("scope:a".into()),
                0,
                through,
                0,
                2048
            )
            .is_err()
    );
}

#[test]
fn metadata_projections_do_not_materialize_context_bodies_and_bound_their_output() {
    use crate::store::database::MetadataField;
    let mut f = Fixture::new();
    f.schemas
        .register_json(r#"{"$id":"arcs.metadata_test.v1","type":"object"}"#)
        .unwrap();
    let mut artifact = f.store.get(&VersionId("input-v1".into())).unwrap().unwrap();
    artifact.artifact_id.0 = "metadata".into();
    artifact.version_id.0 = "metadata-v1".into();
    artifact.schema_id.0 = "arcs.metadata_test.v1".into();
    artifact.artifact_type = "metadata_test".into();
    artifact.stream_key = "metadata".into();
    artifact.payload = serde_json::json!({"raw_text":format!("PRIVATESECRET{}","x".repeat(1_100_000)),"ids":[1,2,3],"private":"metadata_private","budget":{"max":5}});
    f.store.append(&artifact, &f.schemas).unwrap();
    let fields = [
        MetadataField::array_length("count", "$.payload.ids"),
        MetadataField::value("budget", "$.payload.budget"),
        MetadataField::prefix("prefix", "$.payload.raw_text", 5),
    ];
    let projection = f
        .store
        .metadata_projection(&artifact.version_id, &fields, 2048)
        .unwrap()
        .unwrap();
    assert_eq!(projection.fields["count"], 3);
    assert_eq!(projection.fields["budget"]["max"], 5);
    assert_eq!(projection.fields["prefix"], "PRIVA");
    assert!(projection.fields.to_string().len() < 128);
    assert!(!projection.fields.to_string().contains("metadata_private"));
    assert!(
        f.store
            .metadata_projection(&artifact.version_id, &fields, 100)
            .is_err()
    );
    let literal = [MetadataField::value(
        "');DROP TABLE artifact_versions;--",
        "$.payload.budget",
    )];
    assert!(
        f.store
            .metadata_projection(&artifact.version_id, &literal, 2048)
            .unwrap()
            .is_some()
    );
    assert!(f.store.get(&artifact.version_id).unwrap().is_some());
}

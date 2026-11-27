use super::*;
use crate::core::*;
use crate::store::{ArtifactRelations, relation_kinds};
use serde_json::json;

pub(super) fn save(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    id: &str,
    project: &str,
) -> Artifact {
    save_body(store, schemas, id, project, id)
}
pub(super) fn save_body(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    id: &str,
    project: &str,
    body: &str,
) -> Artifact {
    let mut artifact = Artifact::new(
        id,
        format!("{id}-v1"),
        "input",
        "arcs.input.v1",
        "2026-11-15T00:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: id.into(),
        },
        Trust {
            level: TrustLevel::Medium,
            source_class: SourceClass::Human,
        },
        id,
        json!({"raw_text":body}),
    );
    artifact.tags = vec![project.into()];
    artifact.subject = Some(SubjectId(project.into()));
    store.append(&artifact, schemas).unwrap();
    artifact
}
pub(super) fn query(seed: VersionId) -> RetrievalQuery {
    RetrievalQuery {
        seeds: vec![seed],
        scopes: vec![RetrievalScope {
            filter: ArtifactFilter {
                schema_id: SchemaId("arcs.input.v1".into()),
                subject: None,
                tags: vec![],
                max_classification: None,
            },
            text: None,
        }],
        relations: vec![relation_kinds::based_on()],
        limits: RetrievalLimits {
            max_depth: 3,
            max_visited: 32,
            max_edges: 64,
            max_candidates: 16,
        },
    }
}

#[test]
fn metadata_requires_explicit_scope_and_respects_all_filters() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "project:a");
    let b = save(&store, &schemas, "b", "project:b");
    let mut q = query(a.version_id.clone());
    let result = ArtifactRetrieval::new(&store).recall(&q).unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].artifact, a);
    q.scopes[0].filter.subject = Some(SubjectId("project:b".into()));
    q.scopes[0].filter.tags = vec!["project:b".into()];
    let result = ArtifactRetrieval::new(&store).recall(&q).unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].artifact, b);
    q.scopes[0].filter.tags.push("missing".into());
    assert!(
        ArtifactRetrieval::new(&store)
            .recall(&q)
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn graph_is_traversed_in_both_directions_and_cycles_are_bounded() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "a");
    let b = save(&store, &schemas, "b", "b");
    let c = save(&store, &schemas, "c", "c");
    let links = ArtifactRelations::new(&store);
    for (from, to) in [(&a, &b), (&b, &c), (&c, &a)] {
        links
            .connect(
                from.version_id.clone(),
                to.version_id.clone(),
                relation_kinds::based_on(),
            )
            .unwrap();
    }
    let result = ArtifactRetrieval::new(&store)
        .recall(&query(a.version_id))
        .unwrap();
    assert_eq!(result.hits.len(), 3);
    assert_eq!(result.trace.visited, 3);
    assert!(!result.trace.truncated);
    assert_eq!(result.hits[0].distance, 0);
    assert!(
        result.hits[1..]
            .iter()
            .all(|hit| hit.distance == 1 && hit.path.len() == 1)
    );
}

#[test]
fn only_whitelisted_relations_are_followed() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "a");
    let b = save(&store, &schemas, "b", "b");
    ArtifactRelations::new(&store)
        .connect(
            a.version_id.clone(),
            b.version_id,
            relation_kinds::similar_to(),
        )
        .unwrap();
    assert_eq!(
        ArtifactRetrieval::new(&store)
            .recall(&query(a.version_id))
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[test]
fn visit_edge_and_candidate_limits_report_truncation() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "a");
    for id in ["b", "c", "d"] {
        let other = save(&store, &schemas, id, id);
        ArtifactRelations::new(&store)
            .connect(
                a.version_id.clone(),
                other.version_id,
                relation_kinds::based_on(),
            )
            .unwrap();
    }
    for limits in [
        RetrievalLimits {
            max_visited: 1,
            ..query(a.version_id.clone()).limits
        },
        RetrievalLimits {
            max_edges: 1,
            ..query(a.version_id.clone()).limits
        },
        RetrievalLimits {
            max_candidates: 1,
            ..query(a.version_id.clone()).limits
        },
    ] {
        let mut q = query(a.version_id.clone());
        q.limits = limits;
        let result = ArtifactRetrieval::new(&store).recall(&q).unwrap();
        assert!(result.trace.truncated);
        assert!(result.trace.visited <= q.limits.max_visited);
        assert!(result.trace.edges_read <= q.limits.max_edges);
        assert!(result.hits.len() <= q.limits.max_candidates);
    }
}

#[test]
fn invalid_queries_and_missing_seeds_do_not_modify_store() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "a");
    let count = store.len().unwrap();
    assert!(matches!(
        ArtifactRetrieval::new(&store).recall(&query(VersionId("missing".into()))),
        Err(RetrievalError::MissingSeed(_))
    ));
    let mut q = query(a.version_id);
    q.limits.max_visited = 0;
    assert!(matches!(
        ArtifactRetrieval::new(&store).recall(&q),
        Err(RetrievalError::InvalidQuery)
    ));
    assert_eq!(store.len().unwrap(), count);
}

#[test]
fn rank_prefers_short_paths_then_recent_commits_and_preserves_trust() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let a = save(&store, &schemas, "a", "a");
    let b = save(&store, &schemas, "b", "b");
    let c = save(&store, &schemas, "c", "c");
    let links = ArtifactRelations::new(&store);
    links
        .connect(
            a.version_id.clone(),
            b.version_id.clone(),
            relation_kinds::based_on(),
        )
        .unwrap();
    links
        .connect(
            a.version_id.clone(),
            c.version_id.clone(),
            relation_kinds::based_on(),
        )
        .unwrap();
    let result = ArtifactRetrieval::new(&store)
        .recall(&query(a.version_id))
        .unwrap();
    assert_eq!(result.hits[1].artifact, c);
    assert_eq!(result.hits[2].artifact, b);
    assert!(
        result
            .hits
            .iter()
            .all(|hit| hit.artifact.trust.level == TrustLevel::Medium)
    );
}

#[test]
fn graph_and_metadata_search_survive_reopening_sqlite() {
    let directory = std::env::temp_dir().join(format!("arcs-recall-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("store.sqlite");
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::open(&path).unwrap();
    let a = save(&store, &schemas, "a", "a");
    let b = save(&store, &schemas, "b", "b");
    ArtifactRelations::new(&store)
        .connect(
            b.version_id.clone(),
            a.version_id.clone(),
            relation_kinds::based_on(),
        )
        .unwrap();
    let original = ArtifactRetrieval::new(&store)
        .recall(&query(a.version_id.clone()))
        .unwrap();
    drop(store);
    let store = SqliteArtifactStore::open(&path).unwrap();
    let replay = ArtifactRetrieval::new(&store)
        .recall(&query(a.version_id))
        .unwrap();
    assert_eq!(
        original
            .hits
            .iter()
            .map(|hit| &hit.artifact)
            .collect::<Vec<_>>(),
        replay
            .hits
            .iter()
            .map(|hit| &hit.artifact)
            .collect::<Vec<_>>()
    );
    assert_eq!(original.trace.artifact_cursor, replay.trace.artifact_cursor);
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn oversized_payload_is_skipped_without_expanding_the_context_read() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let root = save(&store, &schemas, "root", "root");
    let mut large = save(&store, &schemas, "large", "large");
    large.version = 2;
    large.version_id = VersionId("large-v2".into());
    large.payload = json!({"raw_text":"x".repeat(300_000)});
    store.append(&large, &schemas).unwrap();
    ArtifactRelations::new(&store)
        .connect(
            root.version_id.clone(),
            large.version_id,
            relation_kinds::based_on(),
        )
        .unwrap();
    let result = ArtifactRetrieval::new(&store)
        .recall(&query(root.version_id))
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.trace.visited, 2);
    assert!(result.trace.truncated);
}

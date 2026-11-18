use super::tests::{query, save};
use super::*;
use crate::core::*;
use crate::store::{ArtifactRelations, relation_kinds};

fn search(seed: VersionId, words: &str) -> RetrievalQuery {
    let mut q = query(seed);
    q.scopes[0].filter.tags = vec!["project:p".into()];
    q.scopes[0].text = Some(PayloadTextQuery {
        query: words.into(),
        fields: vec!["raw_text".into()],
    });
    q.limits.max_visited = 3;
    q
}
fn ready(store: &SqliteArtifactStore, query: &RetrievalQuery) -> RetrievalResult {
    for _ in 0..20 {
        match ArtifactRetrieval::new(store).recall(query) {
            Ok(result) => return result,
            Err(RetrievalError::TextIndexPending { .. }) => {}
            Err(error) => panic!("{error:?}"),
        }
    }
    panic!("index did not converge")
}

#[test]
fn lexical_search_finds_old_evidence_beyond_the_metadata_window() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let seed = save(&store, &schemas, "question", "project:q");
    let wanted = save(&store, &schemas, "Überprüfung-Akku", "project:p");
    for i in 0..100 {
        save(&store, &schemas, &format!("noise-{i}"), "project:p");
    }
    save(
        &store,
        &schemas,
        "anderes-Akku-Überprüfung",
        "project:private",
    );
    let q = search(seed.version_id, "AKKU überprüfung");
    assert!(matches!(
        ArtifactRetrieval::new(&store).recall(&q),
        Err(RetrievalError::TextIndexPending { .. })
    ));
    let count = store.len().unwrap();
    let result = ready(&store, &q);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].artifact, wanted);
    assert!(result.trace.visited <= 3);
    assert_eq!(result.trace.text_indexes.len(), 1);
    assert_eq!(
        result.trace.text_indexes[0].indexed_through,
        result.trace.artifact_cursor
    );
    let mut baseline = search(q.seeds[0].clone(), "unused");
    baseline.scopes[0].text = None;
    assert!(
        !ready(&store, &baseline)
            .hits
            .iter()
            .any(|hit| hit.artifact.version_id == wanted.version_id)
    );
    assert_eq!(store.len().unwrap(), count);
}

#[test]
fn graph_reachability_does_not_bypass_text_or_scope_and_operators_are_literal() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let seed = save(&store, &schemas, "question", "project:q");
    let wanted = save(&store, &schemas, "alpha-beta", "project:p");
    let partial = save(&store, &schemas, "alpha", "project:p");
    let private = save(&store, &schemas, "private-alpha-beta", "project:private");
    for node in [&partial, &private] {
        ArtifactRelations::new(&store)
            .connect(
                seed.version_id.clone(),
                node.version_id.clone(),
                relation_kinds::based_on(),
            )
            .unwrap();
    }
    let mut q = search(seed.version_id, "alpha beta");
    q.limits.max_visited = 16;
    let result = ready(&store, &q);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].artifact, wanted);
    q.scopes[0].text.as_mut().unwrap().query = "alpha OR beta".into();
    assert!(ready(&store, &q).hits.is_empty());
    q.scopes[0].filter.tags.clear();
    q.scopes[0].text.as_mut().unwrap().query = "alpha beta".into();
    // Schema + text alone may filter reachable nodes, but cannot globally seed
    // another project without a subject/tag scope or an explicit graph path.
    assert_eq!(ready(&store, &q).hits.len(), 1);
    assert_eq!(ready(&store, &q).hits[0].artifact, private);
}

#[test]
fn index_backfill_reopens_and_new_versions_are_included_without_changing_old_rows() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let path = std::env::temp_dir().join(format!(
        "arcs-text-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteArtifactStore::open(&path).unwrap();
    let seed = save(&store, &schemas, "question", "project:q");
    for i in 0..70 {
        save(&store, &schemas, &format!("evidence-{i}"), "project:p");
    }
    let q = search(seed.version_id, "evidence");
    let progress = match ArtifactRetrieval::new(&store).recall(&q) {
        Err(RetrievalError::TextIndexPending {
            indexed_through, ..
        }) => indexed_through,
        _ => panic!("expected backfill"),
    };
    assert!(progress <= 33);
    drop(store);
    let store = SqliteArtifactStore::open(&path).unwrap();
    let result = ready(&store, &q);
    assert_eq!(result.hits.len(), 2); // one of three visits is the mandatory seed
    let added = save(&store, &schemas, "new-evidence", "project:p");
    let next = ready(&store, &q);
    assert_eq!(next.hits[0].artifact, added);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_text_queries_are_rejected_before_index_creation() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let seed = save(&store, &schemas, "seed", "project:p");
    for (text, fields) in [
        ("".into(), vec!["raw_text".into()]),
        ("***".into(), vec!["raw_text".into()]),
        ("alpha".into(), vec![]),
        ("a ".repeat(33), vec!["raw_text".into()]),
        ("alpha".into(), vec!["raw_text".into(), "raw_text".into()]),
    ] {
        let mut q = search(seed.version_id.clone(), "valid");
        q.scopes[0].text = Some(PayloadTextQuery {
            query: text,
            fields,
        });
        assert!(matches!(
            ArtifactRetrieval::new(&store).recall(&q),
            Err(RetrievalError::InvalidQuery)
        ));
    }
}

#[test]
fn independent_connections_share_one_rebuildable_projection_without_duplicate_documents() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let path = std::env::temp_dir().join(format!(
        "arcs-text-race-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SqliteArtifactStore::open(&path).unwrap();
    let seed = save(&store, &schemas, "question", "project:q");
    for n in 0..70 {
        save(&store, &schemas, &format!("calibration-{n}"), "project:p");
    }
    drop(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let seed = seed.version_id.clone();
            std::thread::spawn(move || {
                let store = SqliteArtifactStore::open(path).unwrap();
                barrier.wait();
                ready(&store, &search(seed, "calibration"))
                    .hits
                    .into_iter()
                    .map(|hit| hit.artifact.version_id)
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut results = threads.into_iter().map(|thread| thread.join().unwrap());
    let first = results.next().unwrap();
    assert_eq!(first, results.next().unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM artifact_text_documents_v1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 71);
    drop(db);
    let store = SqliteArtifactStore::open(&path).unwrap();
    let count = store.len().unwrap();
    store.clear_text_indexes().unwrap();
    let rebuilt = ready(&store, &search(seed.version_id, "calibration"));
    assert_eq!(
        first,
        rebuilt
            .hits
            .iter()
            .map(|hit| hit.artifact.version_id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(store.len().unwrap(), count);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unicode_case_normalization_and_oversized_skips_agree_between_index_and_replay() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let seed = save(&store, &schemas, "question", "project:q");
    let wanted = save(&store, &schemas, "İstanbul-Überprüfung", "project:p");
    super::tests::save_body(&store, &schemas, "large", "project:p", &"z".repeat(270000));
    let result = ready(&store, &search(seed.version_id, "İSTANBUL überprüfung"));
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].artifact, wanted);
    assert!(result.trace.truncated);
    assert_eq!(result.trace.text_indexes[0].skipped_oversized, 1);
}

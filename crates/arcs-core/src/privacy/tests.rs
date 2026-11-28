use super::*;
use crate::core::{Clock, SchemaRegistry};
use crate::store::SqliteArtifactStore;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

/// Verwaltete Uhr, damit der Ablauf einer Freigabe geprüft werden kann.
struct Time {
    seconds: AtomicU64,
}

impl Time {
    fn at(seconds: u64) -> Self {
        Self {
            seconds: AtomicU64::new(seconds),
        }
    }

    fn advance(&self, seconds: u64) {
        self.seconds.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        crate::core::artifact_factory::format_rfc3339(self.seconds.load(Ordering::SeqCst))
    }
}

fn schemas() -> SchemaRegistry {
    SchemaRegistry::with_bundled_schemas().unwrap()
}

fn store() -> SqliteArtifactStore {
    SqliteArtifactStore::in_memory().unwrap()
}

fn spec(secret_id: SecretId) -> SecretGrantSpec {
    SecretGrantSpec {
        secret_id,
        parameter: "FAN_TOKEN".into(),
        purpose: "act".into(),
        resource: "fan:act".into(),
        ttl_seconds: 600,
        max_uses: 2,
    }
}

fn use_() -> SecretUse {
    SecretUse {
        purpose: "act".into(),
        resource: "fan:act".into(),
    }
}

#[test]
fn a_secret_stays_outside_the_artifact_network() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);

    let secret_id = broker.register("s3cr3t-token-value").unwrap();
    let grant = broker.issue(&spec(secret_id)).unwrap();

    // The grant is auditable, but it never carries the value.
    let stored = store
        .current(
            &crate::core::SubjectId(format!("secret_grant:{}", grant.grant_id)),
            &crate::core::SchemaId(GRANT_SCHEMA.into()),
        )
        .unwrap()
        .expect("the grant is stored");
    assert!(!stored.payload.to_string().contains("s3cr3t-token-value"));

    // No artifact of the network carries the value.
    let mut sequence = 0;
    while let Some(committed) = store.committed_after(sequence).unwrap().first() {
        assert!(
            !committed
                .artifact
                .payload
                .to_string()
                .contains("s3cr3t-token-value"),
            "the secret entered the artifact network"
        );
        sequence = committed.sequence;
    }
}

#[test]
fn a_grant_only_serves_its_purpose_and_resource() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token").unwrap();
    broker.issue(&spec(secret_id)).unwrap();

    let wrong_purpose = SecretUse {
        purpose: "request".into(),
        resource: "fan:act".into(),
    };
    assert!(matches!(
        broker.authorize("grant:secret-1-act-fan-act", &wrong_purpose),
        Err(SecretError::PurposeMismatch { .. })
    ));

    let wrong_resource = SecretUse {
        purpose: "act".into(),
        resource: "other:act".into(),
    };
    assert!(matches!(
        broker.authorize("grant:secret-1-act-fan-act", &wrong_resource),
        Err(SecretError::ResourceMismatch { .. })
    ));
}

#[test]
fn an_expired_grant_is_refused_and_marked() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token").unwrap();
    broker.issue(&spec(secret_id)).unwrap();

    time.advance(601);
    assert!(matches!(
        broker.resolve("grant:secret-1-act-fan-act", &use_()),
        Err(SecretError::Expired(_))
    ));
    let grant = broker
        .lookup("grant:secret-1-act-fan-act")
        .unwrap()
        .unwrap();
    assert_eq!(grant.state, GrantState::Expired);
}

#[test]
fn an_exhausted_and_a_revoked_grant_are_refused() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token").unwrap();
    let limited = SecretGrantSpec {
        max_uses: 1,
        ..spec(secret_id.clone())
    };
    broker.issue(&limited).unwrap();
    assert_eq!(
        broker
            .resolve("grant:secret-1-act-fan-act", &use_())
            .unwrap(),
        "token"
    );
    assert!(matches!(
        broker.resolve("grant:secret-1-act-fan-act", &use_()),
        Err(SecretError::Exhausted(_))
    ));

    let other = SecretGrantSpec {
        resource: "fan:other".into(),
        ..spec(secret_id)
    };
    let revoked = broker.issue(&other).unwrap();
    broker.revoke(&revoked.grant_id).unwrap();
    let revoked_use = SecretUse {
        purpose: "act".into(),
        resource: "fan:other".into(),
    };
    assert!(matches!(
        broker.resolve(&revoked.grant_id, &revoked_use),
        Err(SecretError::Revoked(_))
    ));
}

#[test]
fn a_reference_in_an_order_carries_the_value_only_at_the_boundary() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token-value").unwrap();
    broker.issue(&spec(secret_id)).unwrap();

    let order = json!({"token": {"$secret": "grant:secret-1-act-fan-act"}, "speed": 3});
    let outbound = broker.materialize(&order, &use_()).unwrap();
    assert_eq!(outbound["token"], json!("token-value"));
    assert_eq!(outbound["speed"], json!(3));
    // The stored order keeps the reference.
    assert_eq!(
        order["token"],
        json!({"$secret": "grant:secret-1-act-fan-act"})
    );
}

#[test]
fn an_order_without_a_valid_grant_is_not_materialized() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token-value").unwrap();
    broker.issue(&spec(secret_id)).unwrap();

    let order = json!({"token": {"$secret": "grant:unknown"}});
    assert!(matches!(
        broker.materialize(&order, &use_()),
        Err(SecretError::UnknownGrant(_))
    ));
}

#[test]
fn an_answer_that_echoes_a_secret_is_redacted() {
    let schemas = schemas();
    let store = store();
    let vault = SecretVault::in_memory().unwrap();
    let time = Time::at(1_800_000_000);
    let broker = SecretBroker::new(&store, &schemas, &vault, &time);
    let secret_id = broker.register("token-value").unwrap();
    broker.issue(&spec(secret_id)).unwrap();

    let answer = json!({"receipt": "used token-value twice", "items": ["token-value"]});
    let redacted = broker.redact_value(&answer);
    assert_eq!(redacted["receipt"], json!("used «secret» twice"));
    assert_eq!(redacted["items"][0], json!("«secret»"));
    assert!(!broker.redact_text("token-value").contains("token-value"));
}

#[test]
fn a_boundary_denies_every_destination_by_default() {
    let boundary = BoundaryPolicy::default();
    assert!(!boundary.network);
    assert!(matches!(
        boundary.check_destination("api.example.com"),
        Err(BoundaryViolation::NetworkNotAllowed)
    ));
}

#[test]
fn a_boundary_allows_only_the_declared_destinations() {
    let boundary = BoundaryPolicy {
        network: true,
        destinations: vec!["api.example.com".into(), ".internal.example".into()],
        filesystem_roots: vec!["/srv/project".into()],
    };
    assert!(boundary.is_valid());
    assert!(boundary.allows_destination("api.example.com"));
    assert!(boundary.allows_destination("https://api.example.com/v1"));
    assert!(boundary.allows_destination("node.internal.example"));
    assert!(!boundary.allows_destination("api.example.org"));
    assert!(!boundary.allows_destination("evil.com"));
    assert!(matches!(
        boundary.check_destination("api.example.org"),
        Err(BoundaryViolation::DestinationNotAllowed { .. })
    ));
    assert!(boundary.allows_path("/srv/project/src/main.rs"));
    assert!(!boundary.allows_path("/srv/other/file"));
}

#[test]
fn an_invalid_boundary_is_refused() {
    let boundary = BoundaryPolicy {
        network: true,
        destinations: vec!["https://api.example.com/v1".into()],
        filesystem_roots: vec!["relative/path".into()],
    };
    assert!(!boundary.is_valid());
}

#[test]
fn a_destination_marker_is_read_from_the_order() {
    let order = json!({"hub": {"$destination": "api.example.com"}, "speed": 1});
    assert_eq!(
        collect_destinations(&order),
        vec!["api.example.com".to_string()]
    );
    // Other markers are not destinations.
    let other = json!({"hub": {"$secret": "grant:1"}});
    assert!(collect_destinations(&other).is_empty());
}

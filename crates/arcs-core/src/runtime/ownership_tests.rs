use super::*;
use std::cell::RefCell;

thread_local! {
    static NOW: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Time;
impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        NOW.with(|cell| {
            let value = cell.borrow().clone();
            if value.is_empty() {
                "2026-11-28T08:00:00Z".to_string()
            } else {
                value
            }
        })
    }
}

fn set_now(value: &str) {
    NOW.with(|cell| *cell.borrow_mut() = value.to_string());
}

fn schemas() -> SchemaRegistry {
    SchemaRegistry::with_bundled_schemas().unwrap()
}

fn store() -> SqliteArtifactStore {
    SqliteArtifactStore::in_memory().unwrap()
}

#[test]
fn grants_a_task_to_one_owner_and_keeps_it_idempotent() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let lease = service.acquire("goal:7", "host-a", 60).unwrap();
    assert_eq!(lease.fencing_token, 1);
    assert_eq!(lease.owner_id, "host-a");
    assert_eq!(lease.state, LeaseState::Active);

    let again = service.acquire("goal:7", "host-a", 60).unwrap();
    assert_eq!(again.version_id, lease.version_id);
    assert_eq!(again.fencing_token, 1);
}

#[test]
fn refuses_a_second_owner_while_the_lease_is_valid() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    service.acquire("goal:7", "host-a", 60).unwrap();
    let denied = service.acquire("goal:7", "host-b", 60);
    assert!(matches!(denied, Err(OwnershipError::LeaseHeld { .. })));
}

#[test]
fn hands_the_task_over_after_expiry_and_fences_the_old_owner() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let first = service.acquire("goal:7", "host-a", 60).unwrap();
    set_now("2026-11-28T08:05:00Z");

    let second = service.acquire("goal:7", "host-b", 60).unwrap();
    assert_eq!(second.fencing_token, 2);
    assert_eq!(second.owner_id, "host-b");

    // Der zurückgebliebene Eigentümer darf nicht mehr schreiben.
    assert!(matches!(
        service.validate(&first),
        Err(OwnershipError::FencingTokenMismatch {
            expected: 2,
            presented: 1
        })
    ));
    assert!(matches!(
        service.release(&first),
        Err(OwnershipError::FencingTokenMismatch { .. })
    ));
    assert!(service.validate(&second).is_ok());
}

#[test]
fn rejects_an_expired_lease() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let lease = service.acquire("goal:7", "host-a", 60).unwrap();
    set_now("2026-11-28T08:05:00Z");
    assert!(matches!(
        service.validate(&lease),
        Err(OwnershipError::LeaseExpired { .. })
    ));
    assert!(matches!(
        service.renew(&lease, "host-a", 60),
        Err(OwnershipError::LeaseExpired { .. })
    ));
}

#[test]
fn renews_a_lease_without_changing_its_token() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let lease = service.acquire("goal:7", "host-a", 60).unwrap();
    set_now("2026-11-28T08:00:30Z");
    let renewed = service.renew(&lease, "host-a", 120).unwrap();
    assert_eq!(renewed.fencing_token, lease.fencing_token);
    assert_eq!(renewed.version, lease.version + 1);
    assert!(renewed.expires_at > lease.expires_at);
    assert!(service.validate(&renewed).is_ok());
}

#[test]
fn refuses_a_renewal_by_a_foreign_owner() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let lease = service.acquire("goal:7", "host-a", 60).unwrap();
    assert!(matches!(
        service.renew(&lease, "host-b", 60),
        Err(OwnershipError::NotOwner { .. })
    ));
}

#[test]
fn releases_a_lease_and_allows_a_new_owner() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    let lease = service.acquire("goal:7", "host-a", 60).unwrap();
    let released = service.release(&lease).unwrap();
    assert_eq!(released.state, LeaseState::Released);

    let next = service.acquire("goal:7", "host-b", 60).unwrap();
    assert_eq!(next.fencing_token, 2);
    assert_eq!(next.owner_id, "host-b");
}

#[test]
fn keeps_the_history_of_a_task_lease() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    service.acquire("goal:7", "host-a", 60).unwrap();
    set_now("2026-11-28T08:05:00Z");
    service.acquire("goal:7", "host-b", 60).unwrap();

    let history = service.history("goal:7").unwrap();
    let tokens: Vec<u64> = history.iter().map(|lease| lease.fencing_token).collect();
    assert_eq!(tokens, vec![1, 2]);
}

#[test]
fn rejects_unusable_lease_parameters() {
    set_now("2026-11-28T08:00:00Z");
    let schemas = schemas();
    let store = store();
    let service = OwnershipService::new(&store, &schemas, &Time);

    assert!(matches!(
        service.acquire("goal:7", "host-a", 0),
        Err(OwnershipError::Invalid(_))
    ));
    assert!(matches!(
        service.acquire("goal:7", "host-a", MAX_LEASE_SECONDS + 1),
        Err(OwnershipError::Invalid(_))
    ));
    assert!(matches!(
        service.acquire("", "host-a", 60),
        Err(OwnershipError::Invalid(_))
    ));
}

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs_adapter_protocol::{CapabilityRef, DataRead, DataReference};
use arcs_adapter_sdk::Adapter;
use arcs_file_data::FileData;
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    profile: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-file-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("payload.bin"), [0, 1, 255, 2]).unwrap();
        let fixture = Self {
            directory,
            profile: json!({"adapter_id":"files", "resources":[{"id":"payload-v1","media_type":"application/octet-stream","path":"payload.bin"}]}),
        };
        fixture.save();
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn save(&self) {
        std::fs::write(
            self.path("profile.json"),
            serde_json::to_vec(&self.profile).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Result<FileData, String> {
        FileData::open(&self.path("profile.json"), &self.path("source.sqlite"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn read(
    adapter: &mut FileData,
    reference: DataReference,
) -> Result<Vec<u8>, arcs_adapter_sdk::AdapterError> {
    adapter
        .read_data(DataRead {
            capability: CapabilityRef::new("files", "data.observe"),
            reference,
            offset: 0,
            max_bytes: 8192,
        })
        .map(|chunk| chunk.bytes)
}
fn reference() -> DataReference {
    DataReference {
        id: "payload-v1".into(),
        media_type: "application/octet-stream".into(),
        size_bytes: 4,
    }
}

#[test]
fn pending_metadata_replays_and_acknowledged_data_remains_readable_after_restart() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    let pending = adapter.poll().unwrap().unwrap();
    drop(adapter);
    let mut adapter = fixture.open().unwrap();
    assert_eq!(
        adapter.poll().unwrap().unwrap().message_id,
        pending.message_id
    );
    adapter.acknowledge(&pending.message_id).unwrap();
    adapter.acknowledge(&pending.message_id).unwrap();
    assert!(adapter.poll().unwrap().is_none());
    drop(adapter);
    let mut adapter = fixture.open().unwrap();
    assert!(adapter.poll().unwrap().is_none());
    assert_eq!(read(&mut adapter, reference()).unwrap(), vec![0, 1, 255, 2]);
}

#[test]
fn changed_source_bytes_require_a_new_reference_and_old_snapshot_is_retained() {
    let mut fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    std::fs::write(fixture.path("payload.bin"), [9, 8, 7, 6]).unwrap();
    assert_eq!(read(&mut adapter, reference()).unwrap(), vec![0, 1, 255, 2]);
    drop(adapter);
    assert!(fixture.open().err().unwrap().contains("identity conflict"));
    fixture.profile["resources"][0]["id"] = json!("payload-v2");
    fixture.save();
    let mut adapter = fixture.open().unwrap();
    assert_eq!(read(&mut adapter, reference()).unwrap(), vec![0, 1, 255, 2]);
    let mut new_reference = reference();
    new_reference.id = "payload-v2".into();
    assert_eq!(read(&mut adapter, new_reference).unwrap(), vec![9, 8, 7, 6]);
}

#[test]
fn metadata_changes_and_a_database_owned_by_another_adapter_are_rejected() {
    let mut fixture = Fixture::new();
    drop(fixture.open().unwrap());
    fixture.profile["resources"][0]["media_type"] = json!("image/png");
    fixture.save();
    assert!(fixture.open().err().unwrap().contains("identity conflict"));
    fixture.profile["adapter_id"] = json!("other");
    fixture.save();
    assert!(fixture.open().err().unwrap().contains("another adapter"));
}

#[test]
fn references_cannot_read_other_paths_or_change_snapshot_metadata() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    for change in 0..4 {
        let mut reference = reference();
        match change {
            0 => reference.id = "../payload.bin".into(),
            1 => reference.id = "unknown".into(),
            2 => reference.media_type = "image/png".into(),
            _ => reference.size_bytes = 3,
        }
        assert!(read(&mut adapter, reference).is_err());
    }
    assert!(adapter.acknowledge("data:unknown").is_err());
    assert!(adapter.poll().unwrap().is_some());
}

#[test]
fn failed_import_rolls_back_preceding_new_resources() {
    let mut fixture = Fixture::new();
    drop(fixture.open().unwrap());
    std::fs::write(fixture.path("new.bin"), [42]).unwrap();
    std::fs::write(fixture.path("payload.bin"), [42]).unwrap();
    fixture.profile["resources"].as_array_mut().unwrap().insert(
        0,
        json!({"id":"new-v1","media_type":"application/octet-stream","path":"new.bin"}),
    );
    fixture.save();
    assert!(fixture.open().is_err());
    let database = rusqlite::Connection::open(fixture.path("source.sqlite")).unwrap();
    let count: i64 = database
        .query_row("SELECT count(*) FROM resources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn empty_files_are_supported_and_invalid_profiles_fail_before_database_creation() {
    let mut fixture = Fixture::new();
    fixture.profile["resources"][0]["id"] = json!("../invalid");
    fixture.save();
    assert!(fixture.open().is_err());
    assert!(!fixture.path("source.sqlite").exists());
    fixture.profile["resources"][0]["id"] = json!("payload-v1");
    fixture.save();
    std::fs::write(fixture.path("payload.bin"), []).unwrap();
    let mut adapter = fixture.open().unwrap();
    let mut reference = reference();
    reference.size_bytes = 0;
    assert!(read(&mut adapter, reference).unwrap().is_empty());
}

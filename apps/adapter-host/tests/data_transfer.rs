mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arcs::adapters::{
    CapabilityRef,
    data::{DataError, DataReference},
};
use arcs_host::{Host, HostError};
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    config: Value,
    grant: Value,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-host-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bytes = (0..131_073)
            .map(|index| (index % 256) as u8)
            .collect::<Vec<_>>();
        std::fs::write(directory.join("payload.bin"), &bytes).unwrap();
        std::fs::write(directory.join("empty.bin"), []).unwrap();
        let profile = json!({"adapter_id":"example.files","resources":[
            {"id":"binary-v1","media_type":"application/octet-stream","path":"payload.bin"},
            {"id":"empty-v1","media_type":"application/octet-stream","path":"empty.bin"}
        ]});
        std::fs::write(
            directory.join("profile.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        let fixture = Self {
            directory,
            config: json!({"config_version":1,"database":"db/core.sqlite","schemas":[root.join("crates/arcs-adapter-protocol/schemas/data-reference.schema.json")],
                "adapters":[{"program":support::adapter_binary("arcs-file-data"),"profile":"profile.json","args":[{"path":"db/source.sqlite"}],"grant_file":"grant.json"}]}),
            grant: serde_json::from_str(include_str!("../config/grants/data.json")).unwrap(),
            bytes,
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        std::fs::write(
            self.directory.join("host.json"),
            serde_json::to_vec(&self.config).unwrap(),
        )
        .unwrap();
        std::fs::write(
            self.directory.join("grant.json"),
            serde_json::to_vec(&self.grant).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Result<Host, HostError> {
        Host::open(self.directory.join("host.json"))
    }
    fn reference(&self) -> DataReference {
        DataReference {
            id: "binary-v1".into(),
            media_type: "application/octet-stream".into(),
            size_bytes: self.bytes.len() as u64,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn capability() -> CapabilityRef {
    CapabilityRef::new("example.files", "data.observe")
}

#[test]
fn real_file_adapter_streams_binary_and_empty_objects_and_restarts_after_ack() {
    let fixture = Fixture::new();
    let mut host = fixture.open().unwrap();
    host.poll_round().unwrap();
    let report = host.report().unwrap();
    assert_eq!(report.stored_artifacts, 2);
    let reference: DataReference = report
        .estimates
        .iter()
        .find_map(|estimate| {
            let reference: DataReference = serde_json::from_value(estimate.value.clone()).unwrap();
            (reference.id == "binary-v1").then_some(reference)
        })
        .unwrap();
    let mut bytes = Vec::new();
    assert_eq!(
        host.copy_data(&capability(), &reference, &mut bytes)
            .unwrap(),
        fixture.bytes.len() as u64
    );
    assert_eq!(bytes, fixture.bytes);
    let empty = DataReference {
        id: "empty-v1".into(),
        size_bytes: 0,
        ..reference.clone()
    };
    assert_eq!(
        host.copy_data(&capability(), &empty, &mut vec![]).unwrap(),
        0
    );
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open().unwrap();
    host.poll_round().unwrap();
    assert_eq!(host.report().unwrap().stored_artifacts, 2);
    assert_eq!(host.report().unwrap().world_revision, report.world_revision);
    bytes.clear();
    host.copy_data(&capability(), &reference, &mut bytes)
        .unwrap();
    assert_eq!(bytes, fixture.bytes);
    host.shutdown().unwrap();
}

#[test]
fn metadata_observation_does_not_itself_grant_content_access() {
    let mut fixture = Fixture::new();
    fixture.grant["data_limits"] = Value::Null;
    fixture.save();
    let mut host = fixture.open().unwrap();
    host.poll_round().unwrap();
    assert_eq!(host.report().unwrap().stored_artifacts, 2);
    assert!(matches!(
        host.copy_data(&capability(), &fixture.reference(), &mut vec![]),
        Err(HostError::Data(DataError::NotGranted))
    ));
    host.shutdown().unwrap();
}

#[test]
fn configured_object_limit_and_invalid_reference_prevent_content_transfer() {
    let mut fixture = Fixture::new();
    fixture.grant["data_limits"]["max_object_bytes"] = json!(1024);
    fixture.save();
    let mut host = fixture.open().unwrap();
    host.poll_round().unwrap();
    let mut bytes = Vec::new();
    assert!(matches!(
        host.copy_data(&capability(), &fixture.reference(), &mut bytes),
        Err(HostError::Data(DataError::ObjectTooLarge))
    ));
    let mut invalid = fixture.reference();
    invalid.id = "../payload.bin".into();
    assert!(matches!(
        host.copy_data(&capability(), &invalid, &mut bytes),
        Err(HostError::Data(DataError::InvalidReference))
    ));
    assert!(bytes.is_empty());
    host.shutdown().unwrap();
}

#[test]
fn invalid_data_grants_fail_preflight_before_creating_storage() {
    let mut fixture = Fixture::new();
    let valid = fixture.grant.clone();
    for change in 0..3 {
        fixture.grant = valid.clone();
        match change {
            0 => fixture.grant["data_limits"]["max_chunk_bytes"] = json!(8193),
            1 => fixture.grant["data_limits"]["max_object_bytes"] = json!(0),
            _ => fixture.grant["granted_permissions"] = json!(["file.read"]),
        }
        fixture.save();
        assert!(matches!(fixture.open(), Err(HostError::InvalidConfig(_))));
        assert!(!fixture.directory.join("db").exists());
    }
}

#[test]
fn unknown_or_changed_reference_is_rejected_by_the_source() {
    let fixture = Fixture::new();
    for change in 0..3 {
        let host = fixture.open().unwrap();
        let mut reference = fixture.reference();
        match change {
            0 => reference.id = "unknown".into(),
            1 => reference.media_type = "image/png".into(),
            _ => reference.size_bytes -= 1,
        }
        assert!(matches!(
            host.copy_data(&capability(), &reference, &mut vec![]),
            Err(HostError::Data(DataError::Adapter(_)))
        ));
    }
}

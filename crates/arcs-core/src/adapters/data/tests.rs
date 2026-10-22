use super::*;
use crate::adapters::{
    AdapterGrant, AdapterId, CapabilityContract, CapabilityDescriptor, CapabilityId, ProducerClass,
};
use crate::core::{SchemaId, SchemaRegistry, SourceKind, TrustLevel};
use crate::store::SqliteArtifactStore;
use arcs_adapter_protocol::{DATA_REFERENCE_SCHEMA, DATA_REFERENCE_SCHEMA_JSON};
use std::cell::RefCell;

struct Endpoint {
    manifest: AdapterManifest,
    calls: RefCell<Vec<DataRead>>,
    bad: u8,
}
impl DataAdapter for Endpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn read_chunk(&self, request: &DataRead) -> Result<DataChunk, AdapterCallError> {
        self.calls.borrow_mut().push(request.clone());
        let mut chunk = DataChunk {
            reference_id: request.reference.id.clone(),
            offset: request.offset,
            bytes: (0..request.expected_bytes().unwrap())
                .map(|index| ((request.offset + index as u64) % 256) as u8)
                .collect(),
        };
        match self.bad {
            1 => chunk.reference_id = "wrong".into(),
            2 => chunk.offset += 1,
            3 => {
                chunk.bytes.pop();
            }
            4 => chunk.bytes.push(0),
            5 => return Err(AdapterCallError::Timeout),
            _ => {}
        }
        Ok(chunk)
    }
}

fn fixture(limits: Option<DataLimits>) -> (AdapterRegistry, Endpoint, AdapterGrant) {
    let descriptor = |id: &str| CapabilityDescriptor {
        id: CapabilityId(id.into()),
        contract: CapabilityContract::Observe {
            emits: vec![SchemaId(DATA_REFERENCE_SCHEMA.into())],
        },
        required_permissions: vec![],
    };
    let manifest = AdapterManifest {
        protocol_version: 1,
        adapter_id: AdapterId("files".into()),
        adapter_version: "1".into(),
        capabilities: vec![descriptor("data.observe"), descriptor("disabled")],
    };
    let grant = AdapterGrant {
        adapter_id: manifest.adapter_id.clone(),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("data.observe".into())],
        granted_permissions: vec!["data.read".into()],
        assigned_trust: TrustLevel::Low,
        ingress_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes: 1024,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
        data_limits: limits,
    };
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(DATA_REFERENCE_SCHEMA_JSON).unwrap();
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            manifest.clone(),
            grant.clone(),
            &schemas,
            &SqliteArtifactStore::in_memory().unwrap(),
        )
        .unwrap();
    (
        registry,
        Endpoint {
            manifest,
            calls: RefCell::new(vec![]),
            bad: 0,
        },
        grant,
    )
}

fn limits() -> DataLimits {
    DataLimits {
        max_object_bytes: 200_000,
        max_chunk_bytes: 4096,
    }
}
fn capability() -> CapabilityRef {
    CapabilityRef::new("files", "data.observe")
}
fn reference() -> DataReference {
    DataReference {
        id: "binary-v1".into(),
        media_type: "application/octet-stream".into(),
        size_bytes: 131_073,
    }
}

#[test]
fn service_streams_an_object_larger_than_a_frame_under_grant_limits() {
    let (registry, endpoint, _) = fixture(Some(limits()));
    let mut output = Vec::new();
    let reference = reference();
    assert_eq!(
        DataService::new(&registry)
            .copy(&endpoint, &capability(), &reference, &mut output)
            .unwrap(),
        reference.size_bytes
    );
    assert_eq!(
        output,
        (0..reference.size_bytes)
            .map(|index| (index % 256) as u8)
            .collect::<Vec<_>>()
    );
    assert_eq!(endpoint.calls.borrow().len(), 33);
    assert!(
        endpoint
            .calls
            .borrow()
            .iter()
            .all(|call| call.max_bytes == 4096)
    );
}

#[test]
fn absent_data_grant_disabled_capability_and_oversized_objects_are_rejected_before_io() {
    let (registry, endpoint, _) = fixture(None);
    assert!(matches!(
        DataService::new(&registry).copy(&endpoint, &capability(), &reference(), &mut vec![]),
        Err(DataError::NotGranted)
    ));
    assert!(endpoint.calls.borrow().is_empty());
    let (registry, endpoint, _) = fixture(Some(limits()));
    let service = DataService::new(&registry);
    let mut reference = reference();
    reference.size_bytes = limits().max_object_bytes + 1;
    assert!(matches!(
        service.copy(&endpoint, &capability(), &reference, &mut vec![]),
        Err(DataError::ObjectTooLarge)
    ));
    assert!(matches!(
        service.copy(
            &endpoint,
            &CapabilityRef::new("files", "disabled"),
            &reference,
            &mut vec![]
        ),
        Err(DataError::Authorization(_))
    ));
    reference.id = "../secret".into();
    assert!(matches!(
        service.copy(&endpoint, &capability(), &reference, &mut vec![]),
        Err(DataError::InvalidReference)
    ));
    assert!(endpoint.calls.borrow().is_empty());
}

#[test]
fn malformed_chunks_and_transport_failures_do_not_write_unchecked_bytes() {
    for bad in 1..=5 {
        let (registry, mut endpoint, _) = fixture(Some(limits()));
        endpoint.bad = bad;
        let mut output = vec![];
        let result =
            DataService::new(&registry).copy(&endpoint, &capability(), &reference(), &mut output);
        if bad == 5 {
            assert!(matches!(
                result,
                Err(DataError::Adapter(AdapterCallError::Timeout))
            ));
        } else {
            assert!(matches!(result, Err(DataError::InvalidChunk)));
        }
        assert!(output.is_empty());
        assert_eq!(endpoint.calls.borrow().len(), 1);
    }
}

#[test]
fn empty_object_still_checks_the_adapter_and_writer_failure_stops_transfer() {
    let (registry, endpoint, _) = fixture(Some(limits()));
    let mut reference = reference();
    reference.size_bytes = 0;
    assert_eq!(
        DataService::new(&registry)
            .copy(&endpoint, &capability(), &reference, &mut vec![])
            .unwrap(),
        0
    );
    assert_eq!(endpoint.calls.borrow().len(), 1);
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    reference.size_bytes = 10_000;
    assert!(matches!(
        DataService::new(&registry).copy(&endpoint, &capability(), &reference, &mut Broken),
        Err(DataError::Write(_))
    ));
    assert_eq!(endpoint.calls.borrow().len(), 2);
}

#[test]
fn registry_rejects_data_limits_without_permission_and_outside_wire_bounds() {
    let (_, endpoint, mut grant) = fixture(Some(limits()));
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(DATA_REFERENCE_SCHEMA_JSON).unwrap();
    let registry = AdapterRegistry::new();
    grant.granted_permissions.clear();
    assert!(matches!(
        registry.validate_registration(&endpoint.manifest, &grant, &schemas),
        Err(AdapterRegistryError::MissingDataReadPermission)
    ));
    grant.granted_permissions.push("data.read".into());
    for invalid in [
        DataLimits {
            max_object_bytes: 0,
            max_chunk_bytes: 1,
        },
        DataLimits {
            max_object_bytes: 1,
            max_chunk_bytes: 8193,
        },
    ] {
        grant.data_limits = Some(invalid);
        assert!(matches!(
            registry.validate_registration(&endpoint.manifest, &grant, &schemas),
            Err(AdapterRegistryError::InvalidDataLimits)
        ));
    }
}

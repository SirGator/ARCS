use std::collections::{HashMap, HashSet};

use crate::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, CapabilityDescriptor, CapabilityId, CapabilityRef,
    ManifestError, ProducerClass,
};
use crate::core::{MAX_SOURCE_REFERENCE_BYTES, SchemaId, SchemaRegistry, SourceKind};
use crate::store::{SqliteArtifactStore, StoreError};

/// Fehler beim kontrollierten Installieren oder Auflösen eines Adapters.
#[derive(Debug)]
pub enum AdapterRegistryError {
    Manifest(ManifestError),
    DuplicateAdapter(AdapterId),
    GrantAdapterMismatch,
    EmptyGrant,
    InvalidPayloadLimit,
    InvalidExternalReferenceLimit,
    MissingIngressSource,
    UnexpectedIngressSource,
    InternalSourceRequiresSystemProducer,
    MissingReasoningLimits,
    UnexpectedReasoningLimits,
    InvalidReasoningLimits,
    DuplicateGrantCapability(CapabilityId),
    UnknownGrantCapability(CapabilityId),
    DuplicateGrantedPermission(String),
    MissingPermission {
        capability: CapabilityId,
        permission: String,
    },
    UnknownSchema(SchemaId),
    ReasoningOutputMustBeCandidate(SchemaId),
    Store(StoreError),
    UnknownAdapter(AdapterId),
    CapabilityNotEnabled {
        adapter: AdapterId,
        capability: CapabilityId,
    },
}

/// Validierte Kombination aus Adapterbehauptungen und Betreiberrechten.
pub struct RegisteredAdapter {
    manifest: AdapterManifest,
    grant: AdapterGrant,
}

impl RegisteredAdapter {
    pub fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    pub fn grant(&self) -> &AdapterGrant {
        &self.grant
    }
}

/// Core-seitige Registry aller installierten Adapter.
///
/// Die Registry speichert keine Netzwerkverbindungen oder Geheimnisse. Ein
/// späterer HTTP-/stdio-Transport authentifiziert eine Verbindung und bindet
/// sie an den vom Gateway erzeugten opaken `AdapterSession`-Handle.
#[derive(Default)]
pub struct AdapterRegistry {
    adapters: HashMap<AdapterId, RegisteredAdapter>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert eine bereits transportseitig authentifizierte
    /// Slice-Integration nach vollständiger Manifest- und Grant-Prüfung.
    ///
    /// Die Methode speichert keine Verbindung und erzeugt keine globale
    /// Gateway-Session. Der jeweilige Slice besitzt seinen Transport selbst.
    pub fn register(
        &mut self,
        manifest: AdapterManifest,
        grant: AdapterGrant,
        schemas: &SchemaRegistry,
        store: &SqliteArtifactStore,
    ) -> Result<(), AdapterRegistryError> {
        self.validate_registration(&manifest, &grant, schemas)?;
        store
            .bind_schemas(schemas)
            .map_err(AdapterRegistryError::Store)?;
        self.insert_validated(manifest, grant);
        Ok(())
    }

    /// Prüft eine Installation vollständig, ohne den Registry-Zustand zu ändern.
    pub fn validate_registration(
        &self,
        manifest: &AdapterManifest,
        grant: &AdapterGrant,
        schemas: &SchemaRegistry,
    ) -> Result<(), AdapterRegistryError> {
        manifest
            .validate()
            .map_err(AdapterRegistryError::Manifest)?;
        if self.adapters.contains_key(&manifest.adapter_id) {
            return Err(AdapterRegistryError::DuplicateAdapter(
                manifest.adapter_id.clone(),
            ));
        }
        if grant.adapter_id != manifest.adapter_id {
            return Err(AdapterRegistryError::GrantAdapterMismatch);
        }
        if grant.enabled_capabilities.is_empty() {
            return Err(AdapterRegistryError::EmptyGrant);
        }
        if grant.max_payload_bytes == 0 {
            return Err(AdapterRegistryError::InvalidPayloadLimit);
        }
        if grant.max_external_reference_bytes == 0
            || grant.max_external_reference_bytes > MAX_SOURCE_REFERENCE_BYTES
        {
            return Err(AdapterRegistryError::InvalidExternalReferenceLimit);
        }

        let mut enabled = HashSet::new();
        let mut needs_ingress_source = false;
        let mut has_reasoning = false;
        for capability_id in &grant.enabled_capabilities {
            if !enabled.insert(capability_id.clone()) {
                return Err(AdapterRegistryError::DuplicateGrantCapability(
                    capability_id.clone(),
                ));
            }
            let capability = manifest.capability(capability_id).ok_or_else(|| {
                AdapterRegistryError::UnknownGrantCapability(capability_id.clone())
            })?;
            needs_ingress_source |= capability.contract.is_input()
                || capability.contract.is_observation()
                || capability.contract.is_request();
            has_reasoning |= capability.contract.is_reasoning();
            for permission in &capability.required_permissions {
                if !grant
                    .granted_permissions
                    .iter()
                    .any(|granted| granted == permission)
                {
                    return Err(AdapterRegistryError::MissingPermission {
                        capability: capability_id.clone(),
                        permission: permission.clone(),
                    });
                }
            }
        }

        match (needs_ingress_source, grant.ingress_source_kind) {
            (true, None) => return Err(AdapterRegistryError::MissingIngressSource),
            (false, Some(_)) => return Err(AdapterRegistryError::UnexpectedIngressSource),
            (true, Some(SourceKind::Internal)) if grant.producer_class != ProducerClass::System => {
                return Err(AdapterRegistryError::InternalSourceRequiresSystemProducer);
            }
            _ => {}
        }

        match (has_reasoning, grant.reasoning_limits.as_ref()) {
            (true, None) => return Err(AdapterRegistryError::MissingReasoningLimits),
            (false, Some(_)) => return Err(AdapterRegistryError::UnexpectedReasoningLimits),
            (true, Some(limits)) if !limits.all_positive() => {
                return Err(AdapterRegistryError::InvalidReasoningLimits);
            }
            _ => {}
        }

        let mut permissions = HashSet::new();
        for permission in &grant.granted_permissions {
            if !permissions.insert(permission) {
                return Err(AdapterRegistryError::DuplicateGrantedPermission(
                    permission.clone(),
                ));
            }
        }

        for capability in &manifest.capabilities {
            for schema in capability
                .contract
                .accepted_schemas()
                .iter()
                .chain(capability.contract.emitted_schemas())
            {
                if schemas.get(schema).is_none() {
                    return Err(AdapterRegistryError::UnknownSchema(schema.clone()));
                }
            }
            if capability.contract.is_reasoning() {
                for schema in capability.contract.emitted_schemas() {
                    let definition = schemas
                        .get(schema)
                        .ok_or_else(|| AdapterRegistryError::UnknownSchema(schema.clone()))?;
                    if definition.artifact_type != "candidate"
                        && !definition.artifact_type.ends_with("_candidate")
                    {
                        return Err(AdapterRegistryError::ReasoningOutputMustBeCandidate(
                            schema.clone(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Übernimmt eine zuvor vollständig validierte Installation.
    pub(crate) fn insert_validated(&mut self, manifest: AdapterManifest, grant: AdapterGrant) {
        self.adapters.insert(
            manifest.adapter_id.clone(),
            RegisteredAdapter { manifest, grant },
        );
    }

    pub fn get(&self, id: &AdapterId) -> Option<&RegisteredAdapter> {
        self.adapters.get(id)
    }

    /// Löst nur tatsächlich freigeschaltete Fähigkeiten auf.
    pub fn authorized_capability(
        &self,
        adapter_id: &AdapterId,
        capability_id: &CapabilityId,
    ) -> Result<(&RegisteredAdapter, &CapabilityDescriptor), AdapterRegistryError> {
        let adapter = self
            .get(adapter_id)
            .ok_or_else(|| AdapterRegistryError::UnknownAdapter(adapter_id.clone()))?;
        if !adapter
            .grant
            .enabled_capabilities
            .iter()
            .any(|enabled| enabled == capability_id)
        {
            return Err(AdapterRegistryError::CapabilityNotEnabled {
                adapter: adapter_id.clone(),
                capability: capability_id.clone(),
            });
        }
        let capability = adapter.manifest.capability(capability_id).ok_or_else(|| {
            AdapterRegistryError::CapabilityNotEnabled {
                adapter: adapter_id.clone(),
                capability: capability_id.clone(),
            }
        })?;
        Ok((adapter, capability))
    }

    /// Prüft eine vollständig qualifizierte Fähigkeit ohne globale
    /// Namensannahmen.
    pub fn is_enabled_capability(&self, capability: &CapabilityRef) -> bool {
        self.authorized_capability(&capability.adapter_id, &capability.capability_id)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::{ADAPTER_PROTOCOL_VERSION, CapabilityContract, ProducerClass};
    use crate::core::TrustLevel;

    use super::*;

    fn manifest() -> AdapterManifest {
        AdapterManifest {
            protocol_version: ADAPTER_PROTOCOL_VERSION,
            adapter_id: AdapterId("sensor.demo".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("sensor.observe".into()),
                contract: CapabilityContract::Observe {
                    emits: vec![SchemaId("arcs.input.v1".into())],
                },
                required_permissions: vec!["sensor.read".into()],
            }],
        }
    }

    #[test]
    fn manifest_permissions_do_not_grant_permissions() {
        let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        let registry = AdapterRegistry::new();
        let manifest = manifest();
        let grant = AdapterGrant {
            adapter_id: manifest.adapter_id.clone(),
            producer_class: ProducerClass::Adapter,
            enabled_capabilities: vec![CapabilityId("sensor.observe".into())],
            granted_permissions: vec![],
            assigned_trust: TrustLevel::Medium,
            ingress_source_kind: Some(SourceKind::Sensor),
            max_payload_bytes: 1024,
            max_external_reference_bytes: 256,
            reasoning_limits: None,
        };

        assert!(matches!(
            registry.validate_registration(&manifest, &grant, &schemas),
            Err(AdapterRegistryError::MissingPermission { .. })
        ));
    }

    #[test]
    fn external_adapter_cannot_claim_internal_source() {
        let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        let registry = AdapterRegistry::new();
        let manifest = manifest();
        let grant = AdapterGrant {
            adapter_id: manifest.adapter_id.clone(),
            producer_class: ProducerClass::Adapter,
            enabled_capabilities: vec![CapabilityId("sensor.observe".into())],
            granted_permissions: vec!["sensor.read".into()],
            assigned_trust: TrustLevel::Medium,
            ingress_source_kind: Some(SourceKind::Internal),
            max_payload_bytes: 1024,
            max_external_reference_bytes: 256,
            reasoning_limits: None,
        };

        assert!(matches!(
            registry.validate_registration(&manifest, &grant, &schemas),
            Err(AdapterRegistryError::InternalSourceRequiresSystemProducer)
        ));
    }
}

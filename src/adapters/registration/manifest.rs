use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::core::SchemaId;

/// Version des universellen ARCS-Adapterprotokolls.
pub const ADAPTER_PROTOCOL_VERSION: u32 = 1;

/// Stabile Identität eines extern installierten Adapters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdapterId(pub String);

/// Stabile Identität genau einer vom Adapter angebotenen Fähigkeit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(pub String);

/// Eindeutige Referenz auf eine konkrete Fähigkeit einer Installation.
///
/// Eine nackte `CapabilityId` ist nur innerhalb eines Manifests eindeutig.
/// Reasoning- und spätere Execution-Verträge verwenden deshalb immer dieses
/// Paar und können zwei gleich benannte Fähigkeiten nicht verwechseln.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRef {
    pub adapter_id: AdapterId,
    pub capability_id: CapabilityId,
}

impl CapabilityRef {
    pub fn new(adapter_id: impl Into<String>, capability_id: impl Into<String>) -> Self {
        Self {
            adapter_id: AdapterId(adapter_id.into()),
            capability_id: CapabilityId(capability_id.into()),
        }
    }
}

/// Vom Betreiber zugewiesene Sicherheitsklasse des Adapterprozesses.
///
/// Der Typ lebt beim universellen Vertrag, sein konkreter Wert steht jedoch
/// ausschließlich im Core-seitigen `AdapterGrant`, niemals im Manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerClass {
    /// Gewöhnlicher externer Input-, State- oder Transform-Adapter.
    Adapter,
    /// Probabilistisches Modell beziehungsweise LLM.
    Model,
    /// Deterministische, vom Betreiber kontrollierte Systemkomponente.
    System,
    /// Adapter, der bereits autorisierte externe Effekte ausführt.
    Executor,
}

/// Universeller Datenvertrag einer einzelnen Adapterfähigkeit.
///
/// Rollen werden absichtlich pro Fähigkeit statt zusätzlich auf Manifestebene
/// beschrieben. Dadurch können sich `roles`, `accepts` und `emits` nicht in
/// widersprüchlichen Parallelfeldern auseinanderentwickeln.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapabilityContract {
    /// Erzeugt ein einmaliges, aktives Ereignis an der Core-Grenze.
    Input { emits: Vec<SchemaId> },
    /// Beobachtet die Außenwelt und erzeugt Boundary-Payloads.
    Observe { emits: Vec<SchemaId> },
    /// Übersetzt geprüfte Eingaben in neue Candidate- oder Boundary-Payloads.
    Transform {
        accepts: Vec<SchemaId>,
        emits: Vec<SchemaId>,
    },
    /// Beschafft gezielt Daten als Antwort auf ein Core-Artifact.
    Request {
        accepts: Vec<SchemaId>,
        emits: Vec<SchemaId>,
    },
    /// Erzeugt bei unbekannten Situationen Vorschläge aus kuratiertem Kontext.
    Reason { emits: Vec<SchemaId> },
    /// Führt einen vom Core kontrollierten Auftrag aus.
    Act {
        accepts: Vec<SchemaId>,
        emits: Vec<SchemaId>,
        /// Jeder erneute Auftrag mit derselben Core-ID muss dieselbe Wirkung
        /// haben. Ohne diesen Vertrag wird die Fähigkeit nicht installiert.
        idempotent: bool,
    },
    /// Liefert geprüfte Daten an eine externe Darstellung oder Senke.
    Output {
        accepts: Vec<SchemaId>,
        emits: Vec<SchemaId>,
        idempotent: bool,
    },
}

impl CapabilityContract {
    /// Alle Verträge, die diese Fähigkeit als Ausgabe erzeugen darf.
    pub fn emitted_schemas(&self) -> &[SchemaId] {
        match self {
            Self::Input { emits }
            | Self::Observe { emits }
            | Self::Transform { emits, .. }
            | Self::Request { emits, .. }
            | Self::Reason { emits }
            | Self::Act { emits, .. }
            | Self::Output { emits, .. } => emits,
        }
    }

    /// Alle Verträge, die diese Fähigkeit als Eingabe akzeptiert.
    pub fn accepted_schemas(&self) -> &[SchemaId] {
        match self {
            Self::Transform { accepts, .. }
            | Self::Request { accepts, .. }
            | Self::Act { accepts, .. }
            | Self::Output { accepts, .. } => accepts,
            Self::Input { .. } | Self::Observe { .. } | Self::Reason { .. } => &[],
        }
    }

    /// Kennzeichnet einmalige, aktive Ereignisse wie User- oder HTTP-Input.
    pub fn is_input(&self) -> bool {
        matches!(self, Self::Input { .. })
    }

    /// Kennzeichnet den speziellen Reasoning-Port.
    pub fn is_reasoning(&self) -> bool {
        matches!(self, Self::Reason { .. })
    }

    /// Nur Observe-Fähigkeiten dürfen Daten ungefragt in den Core pushen.
    pub fn is_observation(&self) -> bool {
        matches!(self, Self::Observe { .. })
    }

    pub fn is_request(&self) -> bool {
        matches!(self, Self::Request { .. })
    }

    pub fn is_output(&self) -> bool {
        matches!(self, Self::Output { .. })
    }
}

/// Eine Fähigkeit und die dafür erforderlichen Betreiberberechtigungen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub contract: CapabilityContract,
    /// Anforderungen des Adapters, ausdrücklich keine erteilten Rechte.
    #[serde(default)]
    pub required_permissions: Vec<String>,
}

/// Vom Adapter gelieferte, aber vom Core validierte Selbstbeschreibung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterManifest {
    pub protocol_version: u32,
    pub adapter_id: AdapterId,
    pub adapter_version: String,
    pub capabilities: Vec<CapabilityDescriptor>,
}

/// Strukturelle Fehler eines Adaptermanifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedProtocol(u32),
    InvalidAdapterId,
    InvalidAdapterVersion,
    MissingCapabilities,
    InvalidCapabilityId,
    DuplicateCapability(CapabilityId),
    MissingSchemaContract(CapabilityId),
    DuplicateSchema {
        capability: CapabilityId,
        schema: SchemaId,
    },
    InvalidPermission(CapabilityId),
    DuplicatePermission {
        capability: CapabilityId,
        permission: String,
    },
    UnsafeExternalAction(CapabilityId),
}

impl AdapterManifest {
    /// Prüft das Manifest vollständig, bevor es Teil der Registry werden darf.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.protocol_version != ADAPTER_PROTOCOL_VERSION {
            return Err(ManifestError::UnsupportedProtocol(self.protocol_version));
        }
        if !valid_identifier(&self.adapter_id.0) {
            return Err(ManifestError::InvalidAdapterId);
        }
        if self.adapter_version.trim().is_empty()
            || self.adapter_version.chars().any(char::is_whitespace)
        {
            return Err(ManifestError::InvalidAdapterVersion);
        }
        if self.capabilities.is_empty() {
            return Err(ManifestError::MissingCapabilities);
        }

        let mut capability_ids = HashSet::new();
        for capability in &self.capabilities {
            if !valid_identifier(&capability.id.0) {
                return Err(ManifestError::InvalidCapabilityId);
            }
            if !capability_ids.insert(capability.id.clone()) {
                return Err(ManifestError::DuplicateCapability(capability.id.clone()));
            }
            validate_capability(capability)?;
        }
        Ok(())
    }

    /// Sucht eine Fähigkeit anhand ihrer stabilen ID.
    pub fn capability(&self, id: &CapabilityId) -> Option<&CapabilityDescriptor> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == *id)
    }
}

fn validate_capability(capability: &CapabilityDescriptor) -> Result<(), ManifestError> {
    let accepts = capability.contract.accepted_schemas();
    let emits = capability.contract.emitted_schemas();
    let needs_accepts = matches!(
        capability.contract,
        CapabilityContract::Transform { .. }
            | CapabilityContract::Request { .. }
            | CapabilityContract::Act { .. }
            | CapabilityContract::Output { .. }
    );
    if (needs_accepts && accepts.is_empty()) || emits.is_empty() {
        return Err(ManifestError::MissingSchemaContract(capability.id.clone()));
    }

    validate_schema_list(&capability.id, accepts)?;
    validate_schema_list(&capability.id, emits)?;

    let mut permissions = HashSet::new();
    for permission in &capability.required_permissions {
        if !valid_identifier(permission) {
            return Err(ManifestError::InvalidPermission(capability.id.clone()));
        }
        if !permissions.insert(permission) {
            return Err(ManifestError::DuplicatePermission {
                capability: capability.id.clone(),
                permission: permission.clone(),
            });
        }
    }

    if let CapabilityContract::Act { idempotent, .. } = capability.contract
        && (!idempotent || capability.required_permissions.is_empty())
    {
        // Jede Act-Fähigkeit liegt konservativ an einer Effektgrenze. Ein
        // Adapter kann die Schutzpflicht nicht durch `external_effect: false`
        // selbst abschalten.
        return Err(ManifestError::UnsafeExternalAction(capability.id.clone()));
    }
    if let CapabilityContract::Output { idempotent, .. } = capability.contract
        && (!idempotent || capability.required_permissions.is_empty())
    {
        return Err(ManifestError::UnsafeExternalAction(capability.id.clone()));
    }
    Ok(())
}

fn validate_schema_list(
    capability: &CapabilityId,
    schemas: &[SchemaId],
) -> Result<(), ManifestError> {
    let mut seen = HashSet::new();
    for schema in schemas {
        if schema.0.trim().is_empty() || !seen.insert(schema.clone()) {
            return Err(ManifestError::DuplicateSchema {
                capability: capability.clone(),
                schema: schema.clone(),
            });
        }
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_requires_an_emitted_schema_but_accepts_no_schema() {
        let schema = SchemaId("arcs.input.chat.v1".into());
        let contract = CapabilityContract::Input {
            emits: vec![schema.clone()],
        };
        assert!(contract.is_input());
        assert!(!contract.is_observation());
        assert!(contract.accepted_schemas().is_empty());
        assert_eq!(contract.emitted_schemas(), &[schema]);

        let manifest = AdapterManifest {
            protocol_version: ADAPTER_PROTOCOL_VERSION,
            adapter_id: AdapterId("chat.input".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("chat.receive".into()),
                contract: CapabilityContract::Input { emits: vec![] },
                required_permissions: vec![],
            }],
        };

        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::MissingSchemaContract(_))
        ));
    }

    #[test]
    fn rejects_every_action_without_permission_and_idempotency() {
        let manifest = AdapterManifest {
            protocol_version: ADAPTER_PROTOCOL_VERSION,
            adapter_id: AdapterId("robot.arm".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("robot.arm.move".into()),
                contract: CapabilityContract::Act {
                    accepts: vec![SchemaId("arcs.action.robot_arm.v1".into())],
                    emits: vec![SchemaId("arcs.result.robot_arm.v1".into())],
                    idempotent: false,
                },
                required_permissions: vec![],
            }],
        };

        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::UnsafeExternalAction(_))
        ));
    }

    #[test]
    fn rejects_output_without_permission_and_idempotency() {
        let manifest = AdapterManifest {
            protocol_version: ADAPTER_PROTOCOL_VERSION,
            adapter_id: AdapterId("chat.output".into()),
            adapter_version: "1.0.0".into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("chat.deliver".into()),
                contract: CapabilityContract::Output {
                    accepts: vec![SchemaId("arcs.response_candidate.chat.v1".into())],
                    emits: vec![SchemaId("arcs.result.chat.v1".into())],
                    idempotent: false,
                },
                required_permissions: vec![],
            }],
        };

        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::UnsafeExternalAction(_))
        ));
    }
}

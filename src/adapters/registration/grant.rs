use serde::{Deserialize, Serialize};

use super::{AdapterId, CapabilityId, ProducerClass};
use crate::core::{SourceKind, TrustLevel};
use crate::reasoning::ReasoningLimits;

/// Vom Betreiber erteilte Rechte für genau eine Adapterinstallation.
///
/// Das Manifest erklärt nur, was ein Adapter technisch anbietet. Ausschließlich
/// dieser Core-seitige Grant entscheidet, welche Fähigkeiten und
/// Berechtigungen tatsächlich nutzbar sind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterGrant {
    pub adapter_id: AdapterId,
    /// Nur der Betreiber darf diese Autoritätsklasse festlegen.
    pub producer_class: ProducerClass,
    pub enabled_capabilities: Vec<CapabilityId>,
    pub granted_permissions: Vec<String>,
    /// Tatsächlich jedem akzeptierten Ingress-Artifact zugewiesener Trust.
    pub assigned_trust: TrustLevel,
    /// Vom Betreiber festgelegter Herkunftskanal für externe Eingänge.
    ///
    /// Der Adapter darf `Internal` oder einen anderen Kanal nicht pro Nachricht
    /// selbst behaupten.
    pub ingress_source_kind: Option<SourceKind>,
    pub max_payload_bytes: usize,
    pub max_external_reference_bytes: usize,
    /// Absolute Obergrenzen des Betreibers. Ein Request darf sie nur weiter
    /// einschränken, niemals erhöhen.
    pub reasoning_limits: Option<ReasoningLimits>,
}

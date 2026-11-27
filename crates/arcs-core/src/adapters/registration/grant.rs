use arcs_adapter_protocol::DataLimits;
use serde::{Deserialize, Serialize};

use super::{AdapterId, CapabilityId, ProducerClass};
use crate::core::{Classification, SourceKind, TrustLevel};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_limits_v2: Option<arcs_adapter_protocol::ReasoningLimitsV2>,
    /// Ohne ausdrückliches Limit bleibt der Abruf referenzierter Daten gesperrt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_limits: Option<DataLimits>,
    /// Separates Empfangsrecht; Leserechte eines Quelladapters genügen nicht.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_data_limits: Option<arcs_adapter_protocol::InputDataLimits>,
    /// Vom Betreiber erklärte Datenklasse dieses Adapters.
    ///
    /// Die Angabe wirkt in beide Richtungen:
    ///
    /// - Daten, die der Adapter liefert, tragen diese Stufe.
    /// - Daten, die den Adapter erreichen, dürfen diese Stufe nicht
    ///   überschreiten.
    ///
    /// Ohne Angabe gilt die Standardstufe `internal`. Damit bleibt ein
    /// fremder Adaptermäßig auf interne Daten beschränkt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_classification: Option<Classification>,
}

impl AdapterGrant {
    /// Obergrenze der Schutzstufe, die diesen Adapter erreichen darf.
    ///
    /// Ohne ausdrückliche Angabe gilt die Standardstufe. Ein Request darf
    /// diese Grenze nur weiter einschränken, niemals erhöhen.
    pub fn data_classification_ceiling(&self) -> Classification {
        self.data_classification.unwrap_or(Classification::DEFAULT)
    }

    /// Schutzstufe, mit der die Daten dieses Adapters einzustufen sind.
    ///
    /// Sie entspricht der Obergrenze: Der Adapter erhält keine Daten oberhalb
    /// dieser Stufe, und seine Ausgaben gelten höchstens als so sensibel.
    pub fn ingress_classification(&self) -> Classification {
        self.data_classification_ceiling()
    }
}

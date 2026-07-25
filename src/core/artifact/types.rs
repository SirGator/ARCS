use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stabile Identität eines Artefakts über alle seine Versionen hinweg.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArtifactId(pub String);

/// Eindeutige Identität genau einer unveränderlichen Artefaktversion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionId(pub String);

/// Verweist auf den Vertrag, nach dem der Payload validiert wird.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaId(pub String);

/// Autoritätsklasse des Erstellers.
///
/// Die Klasse ist sicherheitsrelevant: Ein Modell kann beispielsweise einen
/// Vorschlag erzeugen, aber niemals als menschlicher Freigeber auftreten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    /// Ein authentifizierter Mensch.
    Human,
    /// Eine deterministische ARCS-Systemkomponente.
    System,
    /// Ein LLM oder anderes probabilistisches Reasoning-System.
    Model,
    /// Ein kontrollierter Adapter, der eine freigegebene Aktion ausführt.
    Executor,
}

/// Identifiziert den konkreten Ersteller eines Artefakts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// Sicherheitsklasse des Erstellers.
    pub actor_type: ActorType,
    /// Innerhalb der Klasse eindeutige Identität.
    pub id: String,
}

/// Kanal, über den eine Information in ARCS gelangt ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Menschliche oder maschinelle Unterhaltung.
    Chat,
    /// Datei oder Dateisystemereignis.
    File,
    /// Externe API-Anfrage.
    Api,
    /// Messwert oder Gerätestatus.
    Sensor,
    /// Zeitgesteuertes Ereignis.
    Timer,
    /// Von ARCS selbst erzeugte Information.
    Internal,
}

/// Nachvollziehbare Herkunft eines Artefakts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Art des Eingangskanals.
    pub kind: SourceKind,
    /// Externe oder interne Referenz auf den konkreten Ursprung.
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Grobe Vertrauensbewertung einer Quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Unbestätigte oder unzuverlässige Information.
    Low,
    /// Plausible, aber nicht vollständig bestätigte Information.
    Medium,
    /// Authentifizierte oder anderweitig stark bestätigte Information.
    High,
}

/// Ursprungsklasse, die unabhängig vom technischen Eingangskanal gilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    /// Von einem Menschen bereitgestellt.
    Human,
    /// Von einer vertrauenswürdigen Systemkomponente erzeugt.
    System,
    /// Von einem probabilistischen Modell vorgeschlagen.
    Model,
    /// Von einem System außerhalb der ARCS-Vertrauensgrenze empfangen.
    External,
}

/// Sicherheitsmetadaten zur Herkunft eines Artefakts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trust {
    /// Aktuell zugewiesenes Vertrauensniveau.
    pub level: TrustLevel,
    /// Klasse des ursprünglichen Informationsgebers.
    pub source_class: SourceClass,
}

/// Reproduzierbare Spur eines an der Erzeugung beteiligten Modells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUse {
    /// Modell- oder Deploymentname.
    pub name: String,
    /// Hash des tatsächlich verwendeten Prompts.
    pub prompt_hash: String,
    /// Referenzen auf die Eingabeartefakte.
    pub inputs: Vec<String>,
    /// Sampling-Temperatur des Aufrufs.
    pub temperature: f64,
    /// Hash der unveränderten Modellausgabe.
    pub raw_output_hash: String,
}

/// Belegt, aus welchen Informationen und Transformationen etwas entstand.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// Direkte Vorgänger im Artefaktgraphen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
    /// Deterministische Regeln, die angewendet wurden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules_applied: Vec<String>,
    /// Probabilistische Modelle, die Vorschläge beigetragen haben.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models_used: Vec<ModelUse>,
    /// Beschreibung einer zusätzlichen Transformation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
}

/// Unveränderlicher, versionierter Umschlag für alle ARCS-Daten.
///
/// Freigabe, Verifikation und Ausführung verändern dieses Objekt niemals.
/// Stattdessen werden neue Artefakte gespeichert, die auf die betreffende
/// Version verweisen. Dadurch bleibt jede Entscheidung rekonstruierbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    /// Stabile Identität über mehrere Versionen hinweg.
    pub artifact_id: ArtifactId,
    /// Eindeutige Identität dieser konkreten Version.
    pub version_id: VersionId,
    /// Fortlaufende Version, beginnend bei eins.
    pub version: u64,
    /// Fachlicher Artefakttyp, beispielsweise `task` oder `approval`.
    #[serde(rename = "type")]
    pub artifact_type: String,
    /// Schema, das den Payload dieses Artefakts beschreibt.
    pub schema_id: SchemaId,
    /// Version des referenzierten Payload-Schemas.
    pub schema_version: u64,
    /// Erstellungszeitpunkt im RFC-3339-Format.
    pub created_at: String,
    /// Nachvollziehbarer Ersteller.
    pub created_by: Actor,
    /// Technischer Ursprung der Information.
    pub source: Source,
    /// Vertrauensbewertung des Ursprungs.
    pub trust: Trust,
    /// Gruppiert zusammengehörige Artefakte und Ereignisse.
    pub stream_key: String,
    /// Optionale Such- und Klassifikationsmerkmale.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Fachlicher, durch `schema_id` kontrollierter Inhalt.
    pub payload: Value,
    /// Optionale, aber auditierbare Entstehungsgeschichte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

impl Artifact {
    /// Erzeugt die erste Version eines neuen Artefakts.
    ///
    /// Die Funktion konstruiert nur den Wert. Vor dem Speichern muss
    /// [`crate::core::validate_artifact`] aufgerufen werden; der Store erledigt
    /// dies automatisch und arbeitet dadurch fail-closed.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: impl Into<String>,
        version_id: impl Into<String>,
        artifact_type: impl Into<String>,
        schema_id: impl Into<String>,
        created_at: impl Into<String>,
        created_by: Actor,
        source: Source,
        trust: Trust,
        stream_key: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            artifact_id: ArtifactId(artifact_id.into()),
            version_id: VersionId(version_id.into()),
            version: 1,
            artifact_type: artifact_type.into(),
            schema_id: SchemaId(schema_id.into()),
            schema_version: 1,
            created_at: created_at.into(),
            created_by,
            source,
            trust,
            stream_key: stream_key.into(),
            tags: Vec::new(),
            payload,
            provenance: None,
        }
    }
}

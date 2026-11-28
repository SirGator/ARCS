//! Formales Prüfen als externes Werkzeug.
//!
//! Die Engine ist ein Adapter: Sie bekommt eine aufgelöste Prüfanfrage und
//! liefert ein Ergebnis. Sie erbt weder Autorität noch Store-Zugriff. Ob ihr
//! Ergebnis als Nachweis gilt, entscheidet der Core.

use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId, SchemaId,
    TransformInvocation, TransformResponse,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use arcs_lean_engine::{LeanConfig, LeanEngine};
use serde::{Deserialize, Serialize};

pub const ADAPTER: &str = "formal.check";
pub const CAPABILITY: &str = "formal.check";
pub const INPUT_SCHEMA: &str = "arcs.formal_check_request.v1";
pub const OUTPUT_SCHEMA: &str = "arcs.formal_check_outcome.v1";

/// Umgebungsvariable für den Pfad der Lean-Werkzeugkette.
pub const BINARY_VARIABLE: &str = "ARCS_LEAN_BINARY";

/// Obergrenze einer einzelnen formalen Prüfung.
const MAX_RUNTIME_MS: u64 = 120_000;

/// Die aufgelöste Prüfanfrage, wie das Werkzeug sie bekommt.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckInput {
    /// Die Obligation, zu der die Prüfung gehört.
    ///
    /// Das Werkzeug prüft nur die Aussage. Die Obligation bindet die
    /// Anfrage an ihren Auftrag und erscheint deshalb in der Herkunft des
    /// Ergebnisses.
    pub obligation_version: String,
    pub statement: String,
    #[serde(default)]
    pub definitions: Vec<arcs_adapter_protocol::formal::FormalSource>,
    #[serde(default)]
    pub assumptions: Vec<arcs_adapter_protocol::formal::FormalAssumption>,
    #[serde(default)]
    pub allowed_axioms: Vec<String>,
    pub toolchain: arcs_adapter_protocol::formal::Toolchain,
    #[serde(default)]
    pub max_runtime_ms: u64,
}

/// Das Ergebnis, wie es der Core entgegennimmt.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckOutcome {
    pub verdict: arcs_adapter_protocol::formal::FormalVerdict,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub used_axioms: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<arcs_adapter_protocol::formal::FormalFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_text: Option<String>,
    pub duration_ms: u64,
}

/// Das formale Werkzeug.
pub struct FormalChecker {
    engine: LeanEngine,
}

impl FormalChecker {
    /// Baut das Werkzeug mit der gefundenen Lean-Werkzeugkette.
    pub fn new() -> Self {
        Self::with_binary(
            std::env::var(BINARY_VARIABLE)
                .unwrap_or_else(|_| "lean".into())
                .into(),
        )
    }

    /// Baut das Werkzeug mit einer bestimmten Lean-Werkzeugkette.
    ///
    /// Ein Pfad, den es nicht gibt, ist kein Fehler des Aufrufers: Die
    /// Engine meldet dann ein nicht verfügbares Werkzeug, und der Core
    /// führt die Handlung in einen Wartezustand statt in einen Fehler.
    pub fn with_binary(binary: std::path::PathBuf) -> Self {
        Self {
            engine: LeanEngine::new(LeanConfig {
                binary,
                max_runtime_ms: MAX_RUNTIME_MS,
                ..LeanConfig::default()
            }),
        }
    }

    /// Die Version der gefundenen Werkzeugkette.
    pub fn toolchain_version(&self) -> String {
        self.engine.toolchain_version()
    }

    /// Prüft eine Aussage und meldet das Ergebnis.
    pub fn check(&self, input: &CheckInput) -> Result<CheckOutcome, AdapterError> {
        check(&self.engine, input)
    }
}

/// Prüft eine Aussage mit der gegebenen Werkzeugkette.
fn check(engine: &LeanEngine, input: &CheckInput) -> Result<CheckOutcome, AdapterError> {
    if input.statement.trim().is_empty() {
        return Err(AdapterError::rejected("empty formal statement"));
    }
    let request = arcs_adapter_protocol::formal::FormalCheckRequest {
        statement: input.statement.clone(),
        // Das Werkzeug sucht den Beweis selbst. Ein Beweisvorschlag wäre
        // eine Behauptung des Aufrufers, kein Ergebnis.
        proof_text: None,
        definitions: input.definitions.clone(),
        assumptions: input.assumptions.clone(),
        allowed_axioms: input.allowed_axioms.clone(),
        toolchain: input.toolchain.clone(),
        max_runtime_ms: input.max_runtime_ms.clamp(1, MAX_RUNTIME_MS),
    };
    // Ein Werkzeug, das nicht laufen kann, ist kein Fehler des Aufrufers: Es
    // meldet ein Ergebnis, das den Core in einen Wartezustand führt.
    let outcome = engine.check(&request).map_err(|error| {
        AdapterError::unavailable(format!("the formal engine failed: {}", error.message))
    })?;
    Ok(CheckOutcome {
        verdict: outcome.verdict,
        used_axioms: outcome.used_axioms,
        findings: outcome.findings,
        proof_text: outcome.proof_text,
        duration_ms: outcome.duration_ms,
    })
}

impl Default for FormalChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for FormalChecker {
    fn manifest(&self) -> AdapterManifest {
        manifest(ADAPTER)
    }
    fn transform(
        &mut self,
        invocation: TransformInvocation,
    ) -> Result<TransformResponse, AdapterError> {
        transform(invocation, ADAPTER, &self.engine)
    }
}

/// Mehrere Installationen desselben Werkzeugs, jeweils eindeutig adressiert.
pub struct FormalCheckerInstance {
    adapter_id: AdapterId,
    checker: FormalChecker,
}

impl FormalCheckerInstance {
    /// Baut eine benannte Instanz mit der gefundenen Werkzeugkette.
    pub fn new(adapter_id: AdapterId) -> Result<Self, AdapterError> {
        Self::with_binary(adapter_id, FormalChecker::new())
    }

    /// Baut eine benannte Instanz mit einer bestimmten Werkzeugkette.
    pub fn with_binary(
        adapter_id: AdapterId,
        checker: FormalChecker,
    ) -> Result<Self, AdapterError> {
        manifest(&adapter_id.0)
            .validate()
            .map_err(|error| AdapterError::rejected(format!("{error:?}")))?;
        Ok(Self {
            adapter_id,
            checker,
        })
    }
}

impl Adapter for FormalCheckerInstance {
    fn manifest(&self) -> AdapterManifest {
        manifest(&self.adapter_id.0)
    }
    fn transform(
        &mut self,
        invocation: TransformInvocation,
    ) -> Result<TransformResponse, AdapterError> {
        transform(invocation, &self.adapter_id.0, &self.checker.engine)
    }
}

fn manifest(adapter_id: &str) -> AdapterManifest {
    AdapterManifest {
        protocol_version: 1,
        adapter_id: AdapterId(adapter_id.into()),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![CapabilityDescriptor {
            // Die Beschreibung nennt die Grundlage der Werkzeugkette. Der
            // Betreiber braucht sie, um einer Obligation zu sagen, welche
            // Axiome ein Nachweis verwenden darf.
            description: Some(arcs_adapter_protocol::CapabilityDescription {
                purpose: "Formale Prüfung einer Obligation mit Lean 4.".into(),
                tags: vec!["formal".into(), "verification".into()],
                preconditions: Vec::new(),
                effects: Vec::new(),
                constraints: vec![format!(
                    "Die Werkzeugkette ruht auf den Axiomen {}. Eine Obligation \
                     muss sie ausdrücklich erlauben.",
                    arcs_lean_engine::DEFAULT_ALLOWED_AXIOMS.join(", ")
                )],
                resources: Vec::new(),
            }),
            id: CapabilityId(CAPABILITY.into()),
            contract: CapabilityContract::Transform {
                accepts: vec![SchemaId(INPUT_SCHEMA.into())],
                emits: vec![SchemaId(OUTPUT_SCHEMA.into())],
            },
            required_permissions: vec!["data.transform".into()],
        }],
    }
}

fn transform(
    invocation: TransformInvocation,
    adapter_id: &str,
    engine: &LeanEngine,
) -> Result<TransformResponse, AdapterError> {
    if invocation.capability.adapter_id.0 != adapter_id
        || invocation.capability.capability_id.0 != CAPABILITY
        || invocation.input_schema_id.0 != INPUT_SCHEMA
        || invocation.output_schema_id.0 != OUTPUT_SCHEMA
    {
        return Err(AdapterError::rejected("formal check contract mismatch"));
    }
    let input: CheckInput = serde_json::from_value(invocation.input_payload)
        .map_err(|error| AdapterError::rejected(error.to_string()))?;
    let payload = serde_json::to_value(check(engine, &input)?)
        .map_err(|error| AdapterError::rejected(error.to_string()))?;
    Ok(TransformResponse {
        // Die Herkunft nennt die Obligation, damit das Ergebnis im Core
        // seinem Auftrag zugeordnet bleibt.
        external_reference: format!(
            "formal-check:{}:{}",
            input.obligation_version, invocation.invocation_id
        ),
        invocation_id: invocation.invocation_id,
        payload,
    })
}

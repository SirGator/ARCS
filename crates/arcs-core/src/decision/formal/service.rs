//! Persistenz und Bewertung formaler Aussagen und Nachweise.
//!
//! Der Dienst schreibt formale Artefakte in das gemeinsame Artifact-Netz und
//! bewertet, ob ein Nachweis für eine konkrete Aktion trägt. Er entscheidet
//! nie selbst, dass etwas bewiesen sei: Das Urteil kommt aus einem externen
//! formalen Werkzeug, und der Dienst prüft nur, ob dieses Urteil als
//! Nachweis zulässig ist.

use serde::{Deserialize, Serialize};

use arcs_adapter_protocol::formal::{
    FormalCheckOutcome, FormalCheckRequest, FormalFinding, FormalFindingVerdict, FormalSource,
    FormalVerdict, Toolchain,
};

use crate::core::{
    Actor, ActorType, Artifact, ArtifactIdGenerator, Clock, Provenance, SchemaId, SchemaRegistry,
    Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::{
    AssumptionDraft, CounterexampleDraft, DefinitionDraft, EvidenceBinding, EvidenceDecision,
    FormalChecker, FormalError, FormalReport, ModelDraft, ObligationDraft, ProofDraft,
    ResourceLimits, TheoremDraft,
};

/// Schema-Verträge der formalen Artefakte.
pub mod schemas {
    pub const DEFINITION: &str = "arcs.formal_definition.v1";
    pub const ASSUMPTION: &str = "arcs.formal_assumption.v1";
    pub const MODEL: &str = "arcs.formal_model.v1";
    pub const OBLIGATION: &str = "arcs.proof_obligation.v1";
    pub const PROOF: &str = "arcs.proof.v1";
    pub const THEOREM: &str = "arcs.theorem.v1";
    pub const COUNTEREXAMPLE: &str = "arcs.counterexample.v1";
    pub const REPORT: &str = "arcs.formal_verification_report.v1";
    pub const CHECK_REQUEST: &str = "arcs.formal_check_request.v1";
    pub const CHECK_OUTCOME: &str = "arcs.formal_check_outcome.v1";
}

/// Erzeuger formaler Artefakte im Core.
const PUBLISHER: &str = "arcs.formal";

/// Obergrenze für eine Aussage oder einen Beweisvorschlag.
const MAX_SOURCE_BYTES: usize = 64 * 1024;

/// Obergrenze für die Zahl aufgeführter Annahmen.
const MAX_ASSUMPTIONS: usize = 32;

#[derive(Debug, Deserialize, Serialize)]
struct DefinitionPayload {
    name: String,
    language: String,
    body: String,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssumptionPayload {
    name: String,
    statement: String,
    origin: String,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelPayload {
    name: String,
    language: String,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    abstraction: Option<String>,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ObligationPayload {
    name: String,
    statement: String,
    assumption_names: Vec<String>,
    definition_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preconditions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    postconditions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invariants: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_limits: Option<ResourceLimits>,
    allowed_axioms: Vec<String>,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProofPayload {
    statement: String,
    assumption_names: Vec<String>,
    definition_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    obligation_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_revision: Option<String>,
    toolchain: Toolchain,
    allowed_axioms: Vec<String>,
    proof_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prover: Option<String>,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TheoremPayload {
    name: String,
    statement: String,
    proof_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    assumption_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_version: Option<String>,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CounterexamplePayload {
    statement: String,
    witness: String,
    toolchain: Toolchain,
    #[serde(skip_serializing_if = "Option::is_none")]
    obligation_version: Option<String>,
    source_versions: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CheckRequestPayload {
    obligation_version: String,
    statement: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    definitions: Vec<arcs_adapter_protocol::formal::FormalSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    assumptions: Vec<arcs_adapter_protocol::formal::FormalAssumption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    allowed_axioms: Vec<String>,
    toolchain: Toolchain,
    max_runtime_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReportPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    obligation_version: Option<String>,
    statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_revision: Option<String>,
    toolchain: Toolchain,
    verdict: FormalVerdict,
    allowed_axioms: Vec<String>,
    used_axioms: Vec<String>,
    findings: Vec<FormalFinding>,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    proof_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_version: Option<String>,
}

/// Hält formale Aussagen, Nachweise und deren Bewertung im Artifact-Netz.
pub struct FormalEvidenceService<'a> {
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> FormalEvidenceService<'a> {
    pub fn new(
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            schemas,
            store,
            ids,
            clock,
        }
    }

    /// Legt eine formale Definition ab.
    pub fn record_definition(&mut self, draft: &DefinitionDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.body)?;
        let payload = DefinitionPayload {
            name: draft.name.clone(),
            language: "lean4".to_owned(),
            body: draft.body.clone(),
            source_versions: versions(&draft.source_versions),
        };
        self.persist(
            schemas::DEFINITION,
            &format!("definition:{}", draft.name),
            &payload,
            &draft.source_versions,
            Vec::new(),
        )
    }

    /// Legt eine formale Annahme ab.
    ///
    /// Die Herkunft bleibt erhalten: Eine ausdrücklich gesetzte Annahme ist
    /// nicht dasselbe wie eine abgeleitete oder importierte.
    pub fn record_assumption(&mut self, draft: &AssumptionDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.statement)?;
        let payload = AssumptionPayload {
            name: draft.name.clone(),
            statement: draft.statement.clone(),
            origin: draft.origin.as_str().to_owned(),
            source_versions: versions(&draft.source_versions),
        };
        self.persist(
            schemas::ASSUMPTION,
            &format!("assumption:{}", draft.name),
            &payload,
            &draft.source_versions,
            Vec::new(),
        )
    }

    /// Legt ein formales Modell ab.
    pub fn record_model(&mut self, draft: &ModelDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.body)?;
        let payload = ModelPayload {
            name: draft.name.clone(),
            language: "lean4".to_owned(),
            body: draft.body.clone(),
            abstraction: draft.abstraction.clone(),
            source_versions: versions(&draft.source_versions),
        };
        self.persist(
            schemas::MODEL,
            &format!("model:{}", draft.name),
            &payload,
            &draft.source_versions,
            Vec::new(),
        )
    }

    /// Legt eine zu beweisende Aussage ab.
    pub fn record_obligation(&mut self, draft: &ObligationDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.statement)?;
        if draft.assumption_names.len() > MAX_ASSUMPTIONS {
            return Err(FormalError::Invalid("too many assumptions".into()));
        }
        let payload = ObligationPayload {
            name: draft.name.clone(),
            statement: draft.statement.clone(),
            assumption_names: draft.assumption_names.clone(),
            definition_names: draft.definition_names.clone(),
            target_version: draft.target_version.as_ref().map(|v| v.0.clone()),
            model_version: draft.model_version.as_ref().map(|v| v.0.clone()),
            plan_revision: draft.plan_revision.clone(),
            goal: draft.goal.clone(),
            preconditions: optional_list(&draft.preconditions),
            postconditions: optional_list(&draft.postconditions),
            invariants: optional_list(&draft.invariants),
            resource_limits: draft.resource_limits.clone(),
            allowed_axioms: draft.allowed_axioms.clone(),
            source_versions: versions(&draft.source_versions),
        };
        self.persist(
            schemas::OBLIGATION,
            &format!("obligation:{}", draft.name),
            &payload,
            &draft.source_versions,
            Vec::new(),
        )
    }

    /// Legt einen Beweisvorschlag ab.
    ///
    /// Ein Vorschlag ist ausdrücklich noch kein Nachweis. Erst der Prüfbericht
    /// entscheidet, ob er als Evidenz gilt.
    pub fn record_proof(&mut self, draft: &ProofDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.statement)?;
        check_source(&draft.proof_text)?;
        if draft.assumption_names.len() > MAX_ASSUMPTIONS {
            return Err(FormalError::Invalid("too many assumptions".into()));
        }
        // Ein Beweisvorschlag darf weder mehr Annahmen noch mehr Axiome
        // verwenden als die Obligation erlaubt. Sonst könnte ein Vorschlag
        // die Bedingungen seiner eigenen Obligation umgehen.
        if let Some(obligation) = &draft.obligation_version {
            let obligation_payload = self.obligation_payload(obligation)?;
            let extra = draft
                .assumption_names
                .iter()
                .filter(|name| !obligation_payload.assumption_names.contains(name))
                .cloned()
                .collect::<Vec<_>>();
            if !extra.is_empty() {
                return Err(FormalError::Invalid(format!(
                    "the proof assumes more than the obligation allows: {}",
                    extra.join(", ")
                )));
            }
            let extra = draft
                .allowed_axioms
                .iter()
                .filter(|axiom| !obligation_payload.allowed_axioms.contains(axiom))
                .cloned()
                .collect::<Vec<_>>();
            if !extra.is_empty() {
                return Err(FormalError::Invalid(format!(
                    "the proof permits axioms the obligation does not allow: {}",
                    extra.join(", ")
                )));
            }
        }
        let payload = ProofPayload {
            statement: draft.statement.clone(),
            assumption_names: draft.assumption_names.clone(),
            definition_names: draft.definition_names.clone(),
            target_version: draft.target_version.as_ref().map(|v| v.0.clone()),
            obligation_version: draft.obligation_version.as_ref().map(|v| v.0.clone()),
            model_version: draft.model_version.as_ref().map(|v| v.0.clone()),
            plan_revision: draft.plan_revision.clone(),
            toolchain: draft.toolchain.clone(),
            allowed_axioms: draft.allowed_axioms.clone(),
            proof_text: draft.proof_text.clone(),
            prover: draft.prover.clone(),
            source_versions: versions(&draft.source_versions),
        };
        let mut relations = Vec::new();
        if let Some(target) = &draft.target_version {
            relations.push((target.clone(), relation_kinds::verifies()));
        }
        self.persist(
            schemas::PROOF,
            &format!("proof:{}", draft.name),
            &payload,
            &draft.source_versions,
            relations,
        )
    }

    /// Legt eine bewiesene Aussage ab.
    pub fn record_theorem(&mut self, draft: &TheoremDraft) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.statement)?;
        let payload = TheoremPayload {
            name: draft.name.clone(),
            statement: draft.statement.clone(),
            proof_version: draft.proof_version.0.clone(),
            assumption_names: optional_list(&draft.assumption_names),
            model_version: draft.model_version.as_ref().map(|v| v.0.clone()),
            source_versions: versions(&draft.source_versions),
        };
        let mut parents = draft.source_versions.clone();
        parents.push(draft.proof_version.clone());
        self.persist(
            schemas::THEOREM,
            &format!("theorem:{}", draft.name),
            &payload,
            &parents,
            Vec::new(),
        )
    }

    /// Legt ein Gegenbeispiel ab.
    pub fn record_counterexample(
        &mut self,
        draft: &CounterexampleDraft,
    ) -> Result<Artifact, FormalError> {
        check_name(&draft.name)?;
        check_source(&draft.statement)?;
        check_source(&draft.witness)?;
        let payload = CounterexamplePayload {
            statement: draft.statement.clone(),
            witness: draft.witness.clone(),
            toolchain: draft.toolchain.clone(),
            obligation_version: draft.obligation_version.as_ref().map(|v| v.0.clone()),
            source_versions: versions(&draft.source_versions),
        };
        let mut parents = draft.source_versions.clone();
        if let Some(obligation) = &draft.obligation_version {
            parents.push(obligation.clone());
        }
        self.persist(
            schemas::COUNTEREXAMPLE,
            &format!("counterexample:{}", draft.name),
            &payload,
            &parents,
            Vec::new(),
        )
    }

    /// Prüft einen Beweisvorschlag mit einem externen formalen Werkzeug.
    ///
    /// Das Werkzeug liefert nur ein Ergebnis. Der Dienst bewertet zusätzlich,
    /// ob das Ergebnis als Nachweis zulässig ist: Ein `sorry`, ein nicht
    /// erlaubtes Axiom, ein Timeout oder ein unprüfbares Ergebnis machen den
    /// Vorschlag zu keinem Nachweis.
    pub fn verify(
        &mut self,
        proof: &VersionId,
        checker: &dyn FormalChecker,
    ) -> Result<Artifact, FormalError> {
        let artifact = self
            .store
            .get(proof)?
            .ok_or_else(|| FormalError::MissingVersion(proof.clone()))?;
        if artifact.schema_id.0 != schemas::PROOF {
            return Err(FormalError::WrongContract(format!(
                "expected {}, got {}",
                schemas::PROOF,
                artifact.schema_id.0
            )));
        }
        let payload: ProofPayload = serde_json::from_value(artifact.payload.clone())?;
        let request = self.check_request(&payload)?;
        let outcome = checker.check(&request)?;
        let obligation = payload
            .obligation_version
            .as_ref()
            .map(|version| VersionId(version.clone()));
        let report = self.assess(
            &payload.statement,
            &payload.assumption_names,
            &payload.allowed_axioms,
            obligation.as_ref(),
            payload.model_version.clone(),
            payload.plan_revision.clone(),
            payload.target_version.clone(),
            outcome,
            request.max_runtime_ms,
        )?;
        self.store_report(Some(proof.clone()), &report, &payload)
    }

    /// Materialisiert die Prüfanfrage einer Obligation.
    ///
    /// Die Anfrage löst die benannten Definitionen und Annahmen auf, damit das
    /// formale Werkzeug eine vollständige, in sich geschlossene Aufgabe
    /// bekommt. Sie ist zugleich die Grenze der übertragenen Information: Was
    /// nicht in der Anfrage steht, erreicht das Werkzeug nicht.
    pub fn prepare_check(
        &mut self,
        obligation: &VersionId,
        max_runtime_ms: u64,
    ) -> Result<Artifact, FormalError> {
        let payload = self.obligation_payload(obligation)?;
        let assumptions = self.assumptions(&payload.assumption_names)?;
        let definitions = self.definitions(&payload.definition_names)?;
        let check = CheckRequestPayload {
            obligation_version: obligation.0.clone(),
            statement: payload.statement.clone(),
            definitions,
            assumptions,
            allowed_axioms: payload.allowed_axioms.clone(),
            toolchain: Toolchain {
                name: "lean4".into(),
                version: "unresolved".into(),
            },
            max_runtime_ms,
        };
        // Eine inhaltsgleiche Anfrage wird wiederverwendet. Nur so bleibt die
        // Prüfung idempotent: Ein erneuter Lauf erzeugt dieselbe Prüfung und
        // damit dasselbe Ergebnis, auch über einen Neustart hinweg.
        let subject = SubjectId(format!("formal:check-request:{}", payload.name));
        if let Some(existing) = self
            .store
            .current(&subject, &SchemaId(schemas::CHECK_REQUEST.into()))?
        {
            let previous: CheckRequestPayload = serde_json::from_value(existing.payload.clone())?;
            if previous.statement == check.statement
                && previous.obligation_version == check.obligation_version
                && previous.allowed_axioms == check.allowed_axioms
                && previous.assumptions == check.assumptions
                && previous.definitions == check.definitions
                && previous.max_runtime_ms == check.max_runtime_ms
            {
                return Ok(existing);
            }
        }
        self.persist(
            schemas::CHECK_REQUEST,
            &format!("check-request:{}", payload.name),
            &check,
            std::slice::from_ref(obligation),
            Vec::new(),
        )
    }

    /// Nimmt das Ergebnis eines externen formalen Werkzeugs entgegen.
    ///
    /// Das Ergebnis wird nicht geglaubt, sondern geprüft: Ein Platzhalter, ein
    /// nicht erlaubtes Axiom, ein Timeout oder ein unvollständiger Bericht
    /// machen den Vorschlag zu keinem Nachweis. Der Bericht entsteht auch dann,
    /// wenn das Werkzeug keinen Beweis vorlegte: Ein fehlgeschlagener oder
    /// widerlegter Nachweis ist ein dauerhafter Befund, kein fehlender.
    pub fn record_outcome(
        &mut self,
        request: &VersionId,
        outcome: FormalCheckOutcome,
        toolchain: Toolchain,
    ) -> Result<Artifact, FormalError> {
        let artifact = self
            .store
            .get(request)?
            .ok_or_else(|| FormalError::MissingVersion(request.clone()))?;
        if artifact.schema_id.0 != schemas::CHECK_REQUEST {
            return Err(FormalError::WrongContract(format!(
                "expected {}, got {}",
                schemas::CHECK_REQUEST,
                artifact.schema_id.0
            )));
        }
        let check: CheckRequestPayload = serde_json::from_value(artifact.payload.clone())?;
        let obligation = VersionId(check.obligation_version.clone());
        let obligation_payload = self.obligation_payload(&obligation)?;
        let assumption_names: Vec<String> = check
            .assumptions
            .iter()
            .map(|assumption| assumption.name.clone())
            .collect();
        let definition_names: Vec<String> = check
            .definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect();

        // Eine Widerlegung ist kein Beweisvorschlag, sondern ein Gegenbeispiel.
        if outcome.verdict == FormalVerdict::Disproved
            && let Some(witness) = outcome
                .proof_text
                .clone()
                .filter(|text| !text.trim().is_empty())
        {
            self.record_counterexample(&CounterexampleDraft {
                name: format!("{}-counterexample", obligation_payload.name),
                statement: check.statement.clone(),
                witness,
                toolchain: toolchain.clone(),
                obligation_version: Some(obligation.clone()),
                source_versions: vec![request.clone()],
            })?;
        }
        // Ein Beweisvorschlag entsteht nur, wenn das Werkzeug einen vorlegte.
        let proof = match outcome.proof_text.clone() {
            Some(proof_text)
                if !proof_text.trim().is_empty() && outcome.verdict == FormalVerdict::Proved =>
            {
                let draft = ProofDraft {
                    name: format!("{}-proof", obligation_payload.name),
                    statement: check.statement.clone(),
                    assumption_names: assumption_names.clone(),
                    definition_names: definition_names.clone(),
                    obligation_version: Some(obligation.clone()),
                    target_version: obligation_payload
                        .target_version
                        .as_ref()
                        .map(|version| VersionId(version.clone())),
                    model_version: obligation_payload
                        .model_version
                        .as_ref()
                        .map(|version| VersionId(version.clone())),
                    plan_revision: obligation_payload.plan_revision.clone(),
                    toolchain: toolchain.clone(),
                    allowed_axioms: check.allowed_axioms.clone(),
                    proof_text,
                    prover: Some(format!("formal-checker:{}", toolchain.name)),
                    source_versions: vec![request.clone()],
                };
                Some(self.record_proof(&draft)?)
            }
            _ => None,
        };
        let report = self.assess(
            &check.statement,
            &assumption_names,
            &check.allowed_axioms,
            Some(&obligation),
            obligation_payload.model_version.clone(),
            obligation_payload.plan_revision.clone(),
            obligation_payload.target_version.clone(),
            outcome,
            check.max_runtime_ms,
        )?;
        self.store_report(
            proof.map(|proof| proof.version_id),
            &report,
            &ProofPayload {
                statement: check.statement.clone(),
                assumption_names,
                definition_names,
                target_version: obligation_payload.target_version.clone(),
                obligation_version: Some(obligation.0.clone()),
                model_version: obligation_payload.model_version.clone(),
                plan_revision: obligation_payload.plan_revision.clone(),
                toolchain,
                allowed_axioms: check.allowed_axioms.clone(),
                proof_text: String::new(),
                prover: None,
                source_versions: vec![request.0.clone()],
            },
        )
    }

    /// Prüft eine Aussage direkt mit einem formalen Werkzeug.
    ///
    /// Der Weg ohne Beweisvorschlag ist der Regelfall: Das Werkzeug sucht
    /// selbst einen Beweis. Ein gefundener Beweis wird als Vorschlag
    /// abgelegt und anschließend unabhängig geprüft.
    pub fn prove(
        &mut self,
        obligation: &VersionId,
        checker: &dyn FormalChecker,
        max_runtime_ms: u64,
    ) -> Result<Artifact, FormalError> {
        let artifact = self
            .store
            .get(obligation)?
            .ok_or_else(|| FormalError::MissingVersion(obligation.clone()))?;
        if artifact.schema_id.0 != schemas::OBLIGATION {
            return Err(FormalError::WrongContract(format!(
                "expected {}, got {}",
                schemas::OBLIGATION,
                artifact.schema_id.0
            )));
        }
        let payload: ObligationPayload = serde_json::from_value(artifact.payload.clone())?;
        let request = self.obligation_request(&payload, max_runtime_ms)?;
        let outcome = checker.check(&request)?;
        let proof_text = outcome
            .proof_text
            .clone()
            .ok_or_else(|| FormalError::Invalid("the checker returned no proof".into()))?;
        let draft = ProofDraft {
            name: payload.name.clone(),
            statement: payload.statement.clone(),
            assumption_names: payload.assumption_names.clone(),
            definition_names: payload.definition_names.clone(),
            obligation_version: Some(obligation.clone()),
            target_version: payload
                .target_version
                .as_ref()
                .map(|v| VersionId(v.clone())),
            model_version: payload
                .model_version
                .as_ref()
                .map(|version| VersionId(version.clone())),
            plan_revision: payload.plan_revision.clone(),
            toolchain: request.toolchain.clone(),
            allowed_axioms: request.allowed_axioms.clone(),
            proof_text,
            prover: Some(format!("formal-checker:{}", request.toolchain.name)),
            source_versions: vec![obligation.clone()],
        };
        let proof = self.record_proof(&draft)?;
        self.verify(&proof.version_id, checker)
    }

    /// Bewertet, ob ein Nachweis für eine konkrete Aktion trägt.
    ///
    /// Ein gültiger, aktueller Nachweis erlaubt die Aktion. Ein fehlender,
    /// veralteter oder abgelehnter Nachweis blockiert sie. War das formale
    /// Werkzeug lediglich nicht verfügbar, entsteht ein Wartezustand: Der
    /// Nachweis ist nachholbar, und ein erneuter Versuch ist vertretbar.
    pub fn decide(
        &self,
        obligation: &VersionId,
        binding: &EvidenceBinding,
    ) -> Result<EvidenceDecision, FormalError> {
        let payload = self.obligation_payload(obligation)?;
        // Der Bezug der Obligation muss zum geforderten Bezug passen. Was die
        // Obligation nicht erklärt, kann ein Nachweis nicht einschließen.
        if let Some(reason) = binding.mismatch(
            payload.target_version.as_deref(),
            payload.plan_revision.as_deref(),
            payload.goal.as_deref(),
        ) {
            return Ok(EvidenceDecision::Blocked(reason.into()));
        }
        let report = match self.latest_report(&payload.name)? {
            Some(report) => report,
            None => {
                return Ok(EvidenceDecision::Blocked(
                    "no formal evidence for the required obligation".into(),
                ));
            }
        };
        if report.obligation_version.as_deref() != Some(obligation.0.as_str()) {
            return Ok(EvidenceDecision::Blocked(
                "the formal evidence belongs to a different obligation version".into(),
            ));
        }
        if report.statement != payload.statement {
            return Ok(EvidenceDecision::Blocked(
                "the formal evidence proves a different statement".into(),
            ));
        }
        if report.model_version != payload.model_version {
            return Ok(EvidenceDecision::Blocked(
                "the formal evidence refers to a different model version".into(),
            ));
        }
        if report.plan_revision != payload.plan_revision {
            return Ok(EvidenceDecision::Blocked(
                "the formal evidence refers to a different plan revision".into(),
            ));
        }
        match report.verdict {
            FormalVerdict::Proved => {
                // Ein gültiger Nachweis braucht einen geprüften Beweisvorschlag.
                let Some(proof) = report
                    .proof_version
                    .as_ref()
                    .map(|version| VersionId(version.clone()))
                else {
                    return Ok(EvidenceDecision::Blocked(
                        "the formal evidence has no verified proof".into(),
                    ));
                };
                // Der Zielbezug ist Teil der Entscheidung: Ein Nachweis für
                // eine andere Aktion trägt nicht.
                if let Some(target) = &binding.target
                    && !self.verifies(&proof, target)?
                {
                    return Ok(EvidenceDecision::Blocked(
                        "the formal evidence verifies a different target".into(),
                    ));
                }
                Ok(EvidenceDecision::Allowed)
            }
            FormalVerdict::Unavailable => Ok(EvidenceDecision::Waiting(
                "the formal checker is currently unavailable".into(),
            )),
            FormalVerdict::Disproved => Ok(EvidenceDecision::Blocked(
                "the formal evidence disproves the required statement".into(),
            )),
            FormalVerdict::Failed => Ok(EvidenceDecision::Blocked(
                "the formal check did not produce a usable proof".into(),
            )),
            FormalVerdict::Rejected => Ok(EvidenceDecision::Blocked(
                "the formal evidence violates the admissibility rules".into(),
            )),
        }
    }

    /// Liest den aktuellen Prüfbericht einer Obligation.
    pub fn latest_report(
        &self,
        obligation_name: &str,
    ) -> Result<Option<FormalReport>, FormalError> {
        let Some(artifact) = self.store.current(
            &SubjectId(format!("formal:report:{obligation_name}")),
            &SchemaId(schemas::REPORT.into()),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(self.parse_report(&artifact)?))
    }

    fn parse_report(&self, artifact: &Artifact) -> Result<FormalReport, FormalError> {
        let payload: ReportPayload = serde_json::from_value(artifact.payload.clone())?;
        Ok(FormalReport {
            target_version: payload.target_version,
            verdict: payload.verdict,
            statement: payload.statement,
            used_axioms: payload.used_axioms,
            allowed_axioms: payload.allowed_axioms,
            findings: payload.findings,
            duration_ms: payload.duration_ms,
            obligation_version: payload.obligation_version,
            model_version: payload.model_version,
            plan_revision: payload.plan_revision,
            proof_version: payload.proof_version,
            request_version: payload.request_version,
            report_version: Some(artifact.version_id.0.clone()),
        })
    }

    /// Prüft, ob ein Nachweis den angegebenen Zielbezug trägt.
    fn verifies(
        &self,
        target_version: &VersionId,
        target: &VersionId,
    ) -> Result<bool, FormalError> {
        Ok(self
            .store
            .outgoing_relations(target_version)?
            .into_iter()
            .any(|relation| relation.to == *target))
    }

    /// Baut die Prüfanfrage zu einem Beweisvorschlag.
    fn check_request(&self, payload: &ProofPayload) -> Result<FormalCheckRequest, FormalError> {
        let assumptions = self.assumptions(&payload.assumption_names)?;
        let definitions = self.definitions(&payload.definition_names)?;
        Ok(FormalCheckRequest {
            statement: payload.statement.clone(),
            proof_text: Some(payload.proof_text.clone()),
            definitions,
            assumptions,
            allowed_axioms: payload.allowed_axioms.clone(),
            toolchain: payload.toolchain.clone(),
            max_runtime_ms: DEFAULT_MAX_RUNTIME_MS,
        })
    }

    /// Baut die Prüfanfrage zu einer Obligation.
    fn obligation_request(
        &self,
        payload: &ObligationPayload,
        max_runtime_ms: u64,
    ) -> Result<FormalCheckRequest, FormalError> {
        let assumptions = self.assumptions(&payload.assumption_names)?;
        let definitions = self.definitions(&payload.definition_names)?;
        Ok(FormalCheckRequest {
            statement: payload.statement.clone(),
            proof_text: None,
            definitions,
            assumptions,
            allowed_axioms: payload.allowed_axioms.clone(),
            toolchain: Toolchain {
                name: "lean4".into(),
                version: "unknown".into(),
            },
            max_runtime_ms,
        })
    }

    /// Liest die Payload einer Obligation und prüft ihren Vertrag.
    fn obligation_payload(&self, obligation: &VersionId) -> Result<ObligationPayload, FormalError> {
        let artifact = self
            .store
            .get(obligation)?
            .ok_or_else(|| FormalError::MissingVersion(obligation.clone()))?;
        if artifact.schema_id.0 != schemas::OBLIGATION {
            return Err(FormalError::WrongContract(format!(
                "expected {}, got {}",
                schemas::OBLIGATION,
                artifact.schema_id.0
            )));
        }
        Ok(serde_json::from_value(artifact.payload.clone())?)
    }

    /// Sammelt die im Beweisraum benannten Annahmen.
    fn assumptions(
        &self,
        names: &[String],
    ) -> Result<Vec<arcs_adapter_protocol::formal::FormalAssumption>, FormalError> {
        let mut assumptions = Vec::new();
        for name in names {
            let artifact = self
                .store
                .current(
                    &SubjectId(format!("formal:assumption:{name}")),
                    &SchemaId(schemas::ASSUMPTION.into()),
                )?
                .ok_or_else(|| FormalError::Invalid(format!("unknown assumption '{name}'")))?;
            let payload: AssumptionPayload = serde_json::from_value(artifact.payload.clone())?;
            assumptions.push(arcs_adapter_protocol::formal::FormalAssumption {
                name: payload.name,
                statement: payload.statement,
            });
        }
        Ok(assumptions)
    }

    /// Sammelt die für die Aussage benötigten Definitionen.
    ///
    /// Die Definitionen werden namentlich aufgelöst. Der Core lädt nicht blind
    /// alle Definitionen: Eine Aussage benennt genau die Definitionen, die sie
    /// verwendet.
    fn definitions(&self, names: &[String]) -> Result<Vec<FormalSource>, FormalError> {
        let mut definitions = Vec::new();
        for name in names {
            let artifact = self
                .store
                .current(
                    &SubjectId(format!("formal:definition:{name}")),
                    &SchemaId(schemas::DEFINITION.into()),
                )?
                .ok_or_else(|| FormalError::Invalid(format!("unknown definition '{name}'")))?;
            let payload: DefinitionPayload = serde_json::from_value(artifact.payload.clone())?;
            definitions.push(FormalSource {
                name: payload.name,
                body: payload.body,
            });
        }
        Ok(definitions)
    }

    /// Bewertet das Ergebnis des formalen Werkzeugs.
    ///
    /// Diese Bewertung ist die eigentliche Zulässigkeitsprüfung. Sie verlangt
    /// einen vorliegenden Beweis, das Fehlen von `sorry`, die Einhaltung der
    /// erlaubten Axiome und einen vollständigen Prüfbericht.
    #[allow(clippy::too_many_arguments)]
    fn assess(
        &self,
        statement: &str,
        declared_assumptions: &[String],
        allowed_axioms: &[String],
        obligation_version: Option<&VersionId>,
        model_version: Option<String>,
        plan_revision: Option<String>,
        target_version: Option<String>,
        outcome: arcs_adapter_protocol::formal::FormalCheckOutcome,
        max_runtime_ms: u64,
    ) -> Result<FormalReport, FormalError> {
        let mut findings = outcome.findings.clone();
        let mut verdict = outcome.verdict;

        let uses_sorry = findings.iter().any(|finding| {
            finding.check == "formal.no_sorry" && finding.verdict == FormalFindingVerdict::Fail
        });
        if uses_sorry && verdict.is_proof() {
            verdict = FormalVerdict::Rejected;
            findings.push(FormalFinding {
                check: "formal.admissible".into(),
                verdict: FormalFindingVerdict::Fail,
                detail: "the proof uses an unproved placeholder".into(),
            });
        }

        // Annahmen sind ausdrücklich gesetzte Axiome. Sie dürfen im Beweis
        // verwendet werden; jedes weitere Axiom braucht eine ausdrückliche
        // Erlaubnis.
        let declared: Vec<&str> = declared_assumptions.iter().map(String::as_str).collect();
        let forbidden: Vec<&String> = outcome
            .used_axioms
            .iter()
            .filter(|axiom| !declared.contains(&axiom.as_str()))
            .filter(|axiom| !allowed_axioms.contains(axiom))
            .collect();
        if !forbidden.is_empty() && verdict.is_proof() {
            verdict = FormalVerdict::Rejected;
            findings.push(FormalFinding {
                check: "formal.allowed_axioms".into(),
                verdict: FormalFindingVerdict::Fail,
                detail: format!(
                    "the proof uses axioms outside the permitted set: {}",
                    forbidden
                        .iter()
                        .map(|axiom| axiom.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }

        if outcome.duration_ms > max_runtime_ms && verdict.is_proof() {
            verdict = FormalVerdict::Rejected;
            findings.push(FormalFinding {
                check: "formal.runtime".into(),
                verdict: FormalFindingVerdict::Fail,
                detail: "the check exceeded the permitted runtime".into(),
            });
        }

        Ok(FormalReport {
            target_version,
            verdict,
            statement: statement.to_owned(),
            used_axioms: outcome.used_axioms,
            allowed_axioms: allowed_axioms.to_vec(),
            findings,
            duration_ms: outcome.duration_ms,
            obligation_version: obligation_version.map(|version| version.0.clone()),
            model_version,
            plan_revision,
            proof_version: None,
            request_version: None,
            report_version: None,
        })
    }

    /// Schreibt den Prüfbericht und bindet ihn an den Nachweis.
    fn store_report(
        &mut self,
        proof: Option<VersionId>,
        report: &FormalReport,
        proof_payload: &ProofPayload,
    ) -> Result<Artifact, FormalError> {
        let obligation_version = proof_payload.obligation_version.clone();
        let obligation_name = match obligation_version.as_ref() {
            Some(version) => self.obligation_name(version)?,
            None => match &proof {
                Some(proof) => format!("proof-{}", proof.0),
                None => return Err(FormalError::Invalid("the report has no anchor".into())),
            },
        };
        let payload = ReportPayload {
            target_version: report.target_version.clone(),
            obligation_version: obligation_version.clone(),
            statement: report.statement.clone(),
            model_version: report.model_version.clone(),
            plan_revision: report.plan_revision.clone(),
            toolchain: proof_payload.toolchain.clone(),
            verdict: report.verdict,
            allowed_axioms: report.allowed_axioms.clone(),
            used_axioms: report.used_axioms.clone(),
            findings: report.findings.clone(),
            duration_ms: report.duration_ms,
            proof_version: proof.as_ref().map(|version| version.0.clone()),
            request_version: report.request_version.clone(),
        };
        let subject = SubjectId(format!("formal:report:{obligation_name}"));
        let mut relations = Vec::new();
        if let Some(proof) = &proof {
            relations.push((proof.clone(), relation_kinds::verifies()));
        }
        if let Some(version) = &obligation_version {
            relations.push((VersionId(version.clone()), relation_kinds::based_on()));
        }
        let parent = match &proof {
            Some(proof) => proof.clone(),
            None => match &obligation_version {
                Some(version) => VersionId(version.clone()),
                None => {
                    return Err(FormalError::Invalid("the report has no anchor".into()));
                }
            },
        };
        let artifact = self.build(schemas::REPORT, &subject, &payload, &parent)?;
        self.store
            .append_current_related_resolved(&artifact, self.schemas, &relations)
            .map_err(FormalError::Store)
    }

    /// Liest den Namen der Obligation, auf die sich ein Nachweis bezieht.
    fn obligation_name(&self, obligation_version: &str) -> Result<String, FormalError> {
        let artifact = self
            .store
            .get(&VersionId(obligation_version.to_owned()))?
            .ok_or_else(|| FormalError::MissingVersion(VersionId(obligation_version.to_owned())))?;
        let payload: ObligationPayload = serde_json::from_value(artifact.payload.clone())?;
        Ok(payload.name)
    }

    /// Legt ein formales Artefakt mit Herkunft und Bezügen ab.
    fn persist(
        &mut self,
        schema_id: &str,
        subject_suffix: &str,
        payload: &impl Serialize,
        parents: &[VersionId],
        relations: Vec<(VersionId, crate::store::RelationKind)>,
    ) -> Result<Artifact, FormalError> {
        let subject = SubjectId(format!("formal:{subject_suffix}"));
        let parent_ids: Vec<String> = parents.iter().map(|v| v.0.clone()).collect();
        let artifact = self.build_with_parents(schema_id, &subject, payload, &parent_ids)?;
        self.store
            .append_current_related_resolved(&artifact, self.schemas, &relations)
            .map_err(FormalError::Store)
    }

    fn build(
        &mut self,
        schema_id: &str,
        subject: &SubjectId,
        payload: &impl Serialize,
        parent: &VersionId,
    ) -> Result<Artifact, FormalError> {
        self.build_with_parents(schema_id, subject, payload, std::slice::from_ref(&parent.0))
    }

    fn build_with_parents(
        &mut self,
        schema_id: &str,
        subject: &SubjectId,
        payload: &impl Serialize,
        parents: &[String],
    ) -> Result<Artifact, FormalError> {
        let schema = self
            .schemas
            .get(&SchemaId(schema_id.into()))
            .ok_or_else(|| FormalError::Invalid(format!("missing schema '{schema_id}'")))?
            .clone();
        let ids = self.ids.next(&schema.artifact_type);
        let artifact = Artifact {
            artifact_id: ids.artifact_id,
            version_id: ids.version_id,
            version: 1,
            artifact_type: schema.artifact_type.clone(),
            schema_id: schema.id.clone(),
            schema_version: schema.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: subject.0.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: subject.0.clone(),
            subject: Some(subject.clone()),
            tags: vec![],
            payload: serde_json::to_value(payload)?,
            provenance: Some(Provenance {
                parents: parents.to_vec(),
                rules_applied: vec!["formal.artifact".into()],
                models_used: vec![],
                transform: Some("formal.artifact".into()),
            }),
            security: Default::default(),
        };
        crate::core::validate_artifact(&artifact, self.schemas)
            .map_err(|error| FormalError::Invalid(format!("invalid formal artifact: {error:?}")))?;
        Ok(artifact)
    }
}

/// Standardmäßige Obergrenze einer formalen Prüfung.
pub const DEFAULT_MAX_RUNTIME_MS: u64 = 120_000;

fn check_name(name: &str) -> Result<(), FormalError> {
    if name.trim().is_empty() || name.len() > 256 {
        return Err(FormalError::Invalid("invalid formal name".into()));
    }
    Ok(())
}

fn check_source(source: &str) -> Result<(), FormalError> {
    if source.trim().is_empty() {
        return Err(FormalError::Invalid("empty formal source".into()));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(FormalError::Invalid("formal source is too large".into()));
    }
    Ok(())
}

fn versions(versions: &[VersionId]) -> Vec<String> {
    versions.iter().map(|version| version.0.clone()).collect()
}

fn optional_list(values: &[String]) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values.to_vec())
    }
}

//! Anbindung der Aufgabenruntime an formale Nachweise.
//!
//! Der Core entscheidet, ob ein formaler Nachweis eine Handlung trägt. Die
//! Runtime holt ihn: Sie fordert die Prüfung über die Adapter-Infrastruktur an,
//! nimmt das Ergebnis entgegen und bewertet es gegen den aktuellen Zielbezug.
//!
//! Der Nachweis wird nicht erzeugt, um eine Handlung freizugeben, sondern um
//! sie zu verhindern: Fehlt er, wirkt die Handlung nicht.

use arcs::adapters::{CapabilityContract, CapabilityRef};
use arcs::core::{SchemaId, SubjectId, SystemClock, VersionId};
use arcs::formal::{
    AssumptionDraft, AssumptionOrigin, DefinitionDraft, EvidenceBinding, EvidenceDecision,
    FormalEvidenceService, ObligationDraft,
};
use arcs::verification::{VerificationFinding, VerificationVerdict};
use serde::{Deserialize, Serialize};

use super::config::invalid;
use super::{HostError, ReasoningRoute, TaskRuntime};
use crate::Host;

/// Schema der aufgelösten formalen Prüfanfrage.
pub const CHECK_REQUEST_SCHEMA: &str = "arcs.formal_check_request.v1";

/// Schema des Ergebnisses einer formalen Prüfung.
pub const CHECK_OUTCOME_SCHEMA: &str = "arcs.formal_check_outcome.v1";

/// Vorgeschriebener formaler Nachweis für die Handlung einer Zielrunde.
///
/// Die Anforderung benennt die Obligation, die geprüft werden muss, und die
/// Fähigkeit, die die Prüfung ausführt. Der Nachweis selbst entsteht im
/// Core-Artefaktnetz und wird dort bewertet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalEvidenceRequirement {
    /// Name der Obligation im Core-Artefaktnetz.
    pub obligation: String,
    /// Fähigkeit, die die formale Prüfung ausführt.
    pub capability: CapabilityRef,
    /// Harte Obergrenze eines einzelnen Prüflaufs.
    #[serde(default = "default_max_runtime_ms")]
    pub max_runtime_ms: u64,
}

fn default_max_runtime_ms() -> u64 {
    120_000
}

/// Die Herkunft einer Annahme ist ohne Angabe eine gesetzte Annahme.
fn default_assumption_origin() -> AssumptionOrigin {
    AssumptionOrigin::User
}

/// Der Zustand des geforderten Nachweises.
pub enum FormalState {
    /// Der Nachweis liegt vor und trägt die Handlung.
    Ready { report: VersionId },
    /// Der Nachweis ist noch nicht verfügbar; die Handlung wartet.
    Pending(String),
    /// Der Nachweis fehlt dauerhaft oder ist ungültig; die Handlung wird blockiert.
    Rejected(String),
}

/// Der Zugang zu den formalen Nachweisen einer Aufgabe.
impl TaskRuntime {
    /// Löst eine Obligation über ihren Namen auf.
    ///
    /// Der Name ist der Subject des Artefakts. Damit bleibt die Verbindung
    /// zwischen Konfiguration und Nachweis im Core nachvollziehbar.
    pub(super) fn formal_obligation(
        &self,
        host: &Host,
        name: &str,
    ) -> Result<Option<VersionId>, HostError> {
        let artifact = host
            .store()
            .current(
                &SubjectId(format!("formal:obligation:{name}")),
                &SchemaId("arcs.proof_obligation.v1".into()),
            )
            .map_err(HostError::Store)?;
        Ok(artifact.map(|artifact| artifact.version_id))
    }

    /// Der Bezug, unter dem ein Nachweis für diese Aufgabe gilt.
    fn formal_binding(&self, route: &ReasoningRoute) -> EvidenceBinding {
        EvidenceBinding::plan(None, Some(route.id.clone()))
    }

    /// Bewertet den geforderten Nachweis.
    ///
    /// Die Bewertung liest nur: Sie beantwortet die Frage, ob der Nachweis
    /// die Handlung trägt. Das Anstoßen der Prüfung ist eine eigene Sache,
    /// weil es einen Adapteraufruf und damit eine Wirkung außerhalb des
    /// Core bedeutet.
    pub(super) fn formal_evidence_state(
        &mut self,
        host: &Host,
        requirement: &FormalEvidenceRequirement,
        route: &ReasoningRoute,
    ) -> Result<FormalState, HostError> {
        let Some(obligation) = self.formal_obligation(host, &requirement.obligation)? else {
            return Ok(FormalState::Rejected(format!(
                "the required formal obligation '{}' does not exist",
                requirement.obligation
            )));
        };
        let binding = self.formal_binding(route);
        let decision = FormalEvidenceService::new(
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .decide(&obligation, &binding)
        .map_err(formal_error)?;
        match decision {
            EvidenceDecision::Allowed => {
                let name = self.formal_obligation_name(host, &obligation)?;
                let report = FormalEvidenceService::new(
                    host.schemas(),
                    host.store(),
                    &mut self.journal.ids,
                    &SystemClock,
                )
                .latest_report(&name)
                .map_err(formal_error)?;
                match report.and_then(|report| report.report_version) {
                    Some(version) => Ok(FormalState::Ready {
                        report: VersionId(version),
                    }),
                    None => Ok(FormalState::Rejected(
                        "the formal evidence has no report".into(),
                    )),
                }
            }
            EvidenceDecision::Waiting(reason) => Ok(FormalState::Pending(reason)),
            EvidenceDecision::Blocked(reason) => {
                // Ohne jeden Bericht wurde die Prüfung noch nicht ausgeführt.
                // Das ist ein Wartezustand, kein Befund.
                let obligation_name = self.formal_obligation_name(host, &obligation)?;
                let report = FormalEvidenceService::new(
                    host.schemas(),
                    host.store(),
                    &mut self.journal.ids,
                    &SystemClock,
                )
                .latest_report(&obligation_name)
                .map_err(formal_error)?;
                if report.is_none() {
                    return Ok(FormalState::Pending(
                        "the formal evidence has not been produced yet".into(),
                    ));
                }
                Ok(FormalState::Rejected(reason))
            }
        }
    }

    /// Fordert die formale Prüfung an und nimmt ein fertiges Ergebnis entgegen.
    ///
    /// Die Prüfung läuft über die Adapter-Infrastruktur. Sie ist idempotent:
    /// Dieselbe Anfrage ergibt denselben Aufruf und damit dasselbe Ergebnis.
    pub(super) fn request_formal_check(
        &mut self,
        host: &mut Host,
        requirement: &FormalEvidenceRequirement,
        obligation: &VersionId,
    ) -> Result<Option<VersionId>, HostError> {
        let request = FormalEvidenceService::new(
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .prepare_check(obligation, requirement.max_runtime_ms)
        .map_err(formal_error)?;
        // Ein nicht verfügbares Werkzeug ist kein Fehler der Handlung: Die
        // Prüfung wartet dann, bis sie erneut angefordert wird. Erst eine
        // verweigerte oder fehlerhafte Prüfung ist ein Fehler.
        let outcome = match host.poll_transform(
            &requirement.capability,
            &request.version_id,
            &SchemaId(CHECK_OUTCOME_SCHEMA.into()),
            self.config.max_concurrent_transforms,
        ) {
            Ok(outcome) => outcome,
            Err(HostError::Transform {
                source:
                    arcs::transform::TransformError::AdapterCall(
                        arcs::adapters::AdapterCallError::Unavailable(_)
                        | arcs::adapters::AdapterCallError::Timeout,
                    ),
                ..
            }) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let Some(outcome) = outcome else {
            return Ok(None);
        };
        let toolchain = self.formal_toolchain(host, &requirement.capability)?;
        let report = self.record_formal_outcome(host, &outcome.version_id, toolchain)?;
        Ok(Some(report))
    }

    /// Der Befund zum geforderten formalen Nachweis.
    ///
    /// Der Befund benennt die Obligation und den Bericht, auf dem er beruht.
    /// Damit ist im Prüfbericht nachvollziehbar, welcher Nachweis die
    /// Handlung getragen hat.
    pub(super) fn formal_finding(
        &mut self,
        host: &Host,
        requirement: &FormalEvidenceRequirement,
        route: &ReasoningRoute,
    ) -> Result<VerificationFinding, HostError> {
        match self.formal_evidence_state(host, requirement, route)? {
            FormalState::Ready { report } => Ok(VerificationFinding {
                check: "goal.formal_evidence".into(),
                verdict: VerificationVerdict::Pass,
                detail: format!(
                    "the formal obligation '{}' is proven by report {}",
                    requirement.obligation, report.0
                ),
            }),
            FormalState::Pending(reason) => Ok(VerificationFinding {
                check: "goal.formal_evidence".into(),
                verdict: VerificationVerdict::Fail,
                detail: reason,
            }),
            FormalState::Rejected(reason) => Ok(VerificationFinding {
                check: "goal.formal_evidence".into(),
                verdict: VerificationVerdict::Fail,
                detail: reason,
            }),
        }
    }

    /// Die Werkzeugkette, die den Nachweis erbracht hat.
    fn formal_toolchain(
        &self,
        host: &Host,
        capability: &CapabilityRef,
    ) -> Result<arcs::formal::Toolchain, HostError> {
        let (registered, _) = host
            .registry()
            .authorized_capability(&capability.adapter_id, &capability.capability_id)
            .map_err(HostError::Registration)?;
        Ok(arcs::formal::Toolchain {
            name: capability.capability_id.0.clone(),
            version: registered.manifest().adapter_version.clone(),
        })
    }

    /// Nimmt das Ergebnis einer formalen Prüfung entgegen.
    ///
    /// Das Ergebnis wird bewertet, nicht geglaubt: Der Core entscheidet, ob
    /// es als Nachweis trägt.
    pub(super) fn record_formal_outcome(
        &mut self,
        host: &mut Host,
        outcome_version: &VersionId,
        toolchain: arcs::formal::Toolchain,
    ) -> Result<VersionId, HostError> {
        let artifact = host
            .store()
            .get(outcome_version)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("the formal outcome is missing"))?;
        if artifact.schema_id.0 != CHECK_OUTCOME_SCHEMA {
            return Err(invalid("the formal outcome has an unexpected contract"));
        }
        let outcome: arcs::formal::FormalCheckOutcome =
            serde_json::from_value(artifact.payload.clone()).map_err(formal_error)?;
        // Die Prüfanfrage geht dem Ergebnis voraus; sie benennt die
        // Obligation, zu der das Ergebnis gehört.
        let request = artifact
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.parents.first())
            .map(|parent| VersionId(parent.clone()))
            .ok_or_else(|| invalid("the formal outcome has no originating request"))?;
        let report = FormalEvidenceService::new(
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .record_outcome(&request, outcome, toolchain)
        .map_err(formal_error)?;
        Ok(report.version_id)
    }

    /// Liest den Namen der Obligation, zu der ein Artefakt gehört.
    fn formal_obligation_name(
        &self,
        host: &Host,
        obligation: &VersionId,
    ) -> Result<String, HostError> {
        let artifact = host
            .store()
            .get(obligation)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("the formal obligation is missing"))?;
        Ok(artifact
            .payload
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_owned())
    }
}

/// Übersetzt einen Core-Fehler in einen Host-Fehler.
fn formal_error(error: impl std::fmt::Debug) -> HostError {
    invalid(format!("formal evidence: {error:?}"))
}

/// Legt eine formale Obligation samt Definitionen und Annahmen an.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalObligationRequest {
    pub name: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definitions: Vec<FormalDefinitionInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<FormalAssumptionInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_axioms: Vec<String>,
}

/// Eine formale Definition als Eingabe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalDefinitionInput {
    pub name: String,
    pub body: String,
}

/// Eine formale Annahme als Eingabe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormalAssumptionInput {
    pub name: String,
    pub statement: String,
    #[serde(default = "default_assumption_origin")]
    pub origin: AssumptionOrigin,
}

impl TaskRuntime {
    /// Legt eine formale Obligation an.
    ///
    /// Definitionen und Annahmen entstehen als eigene Artefakte. Die
    /// Obligation benennt sie nur; so bleiben ihre Herkunft und ihr jeweils
    /// eigener Schutzgrad nachvollziehbar.
    pub fn register_formal_obligation(
        &mut self,
        host: &Host,
        request: &FormalObligationRequest,
    ) -> Result<VersionId, HostError> {
        let mut sources = Vec::new();
        for definition in &request.definitions {
            let artifact = FormalEvidenceService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .record_definition(&DefinitionDraft {
                name: definition.name.clone(),
                body: definition.body.clone(),
                source_versions: Vec::new(),
            })
            .map_err(formal_error)?;
            sources.push(artifact.version_id);
        }
        for assumption in &request.assumptions {
            let artifact = FormalEvidenceService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .record_assumption(&AssumptionDraft {
                name: assumption.name.clone(),
                statement: assumption.statement.clone(),
                origin: assumption.origin,
                source_versions: Vec::new(),
            })
            .map_err(formal_error)?;
            sources.push(artifact.version_id);
        }
        let obligation = FormalEvidenceService::new(
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .record_obligation(&ObligationDraft {
            name: request.name.clone(),
            statement: request.statement.clone(),
            assumption_names: request
                .assumptions
                .iter()
                .map(|assumption| assumption.name.clone())
                .collect(),
            definition_names: request
                .definitions
                .iter()
                .map(|definition| definition.name.clone())
                .collect(),
            target_version: None,
            model_version: None,
            plan_revision: request.plan_revision.clone(),
            goal: request.goal.clone(),
            preconditions: Vec::new(),
            postconditions: Vec::new(),
            invariants: Vec::new(),
            resource_limits: None,
            allowed_axioms: request.allowed_axioms.clone(),
            source_versions: sources,
        })
        .map_err(formal_error)?;
        Ok(obligation.version_id)
    }
}

impl TaskRuntime {
    /// Führt einen Befehl der formalen Nachweise aus.
    ///
    /// Das Anlegen einer Obligation und das Anstoßen einer Prüfung sind
    /// Wirkungen des Betreibers. Sie werden hier gebündelt, damit der
    /// nachvollziehbare Weg von der Konfiguration zum Nachweis derselbe ist.
    pub fn apply_formal(
        &mut self,
        host: &mut Host,
        command: crate::operator_command::FormalCommand,
    ) -> Result<crate::operator_command::FormalReceipt, HostError> {
        use crate::operator_command::{FormalCommand, FormalReceipt};
        match command {
            FormalCommand::Obligate { request } => {
                let obligation = self.register_formal_obligation(&*host, &request)?;
                Ok(FormalReceipt {
                    obligation_version_id: Some(obligation),
                    report_version_id: None,
                    state: "obligated".into(),
                })
            }
            FormalCommand::Prove {
                obligation,
                requirement,
            } => {
                let Some(version) = self.formal_obligation(&*host, &obligation)? else {
                    return Err(invalid(format!(
                        "the formal obligation '{obligation}' does not exist"
                    )));
                };
                let report = self.request_formal_check(host, &requirement, &version)?;
                let verified = report.is_some();
                Ok(FormalReceipt {
                    obligation_version_id: Some(version),
                    report_version_id: report,
                    state: if verified { "verified" } else { "pending" }.into(),
                })
            }
        }
    }
}

/// Prüft, ob eine Fähigkeit die formale Prüfung tragen kann.
pub fn formal_capability(host: &Host, capability: &CapabilityRef) -> Result<(), HostError> {
    let (_, descriptor) = host
        .registry()
        .authorized_capability(&capability.adapter_id, &capability.capability_id)
        .map_err(HostError::Registration)?;
    match &descriptor.contract {
        CapabilityContract::Transform { accepts, emits } => {
            if accepts
                .iter()
                .any(|schema| schema.0 == CHECK_REQUEST_SCHEMA)
                && emits.iter().any(|schema| schema.0 == CHECK_OUTCOME_SCHEMA)
            {
                Ok(())
            } else {
                Err(invalid(
                    "the formal capability must accept a check request and emit a check outcome",
                ))
            }
        }
        _ => Err(invalid("the formal capability must be a transform")),
    }
}

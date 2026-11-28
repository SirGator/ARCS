//! Verträge der kontrollierten Autonomie.
//!
//! Autonomie ist eine Eigenschaft der *Betreiberregel*, nicht des Vorschlags.
//! Ein Vorschlag darf Risiko und Kosten behaupten; verbindlich sind erst die
//! konservativ zusammengeführten Werte aus Betreiberregel und Vorschlag.
//!
//! Die drei Stufen aus BIZ-05 bilden eine absteigende Erlaubniskette:
//!
//! - `Routine`: risikoarme, ausdrücklich erlaubte Arbeit ohne Geldbetrag.
//! - `Bounded`: bedingt autonome Arbeit innerhalb festgelegter Preis-,
//!   Budget- und Risikogrenzen.
//! - `HumanDecision`: ein Mensch entscheidet. Dieser Wert ist der sichere
//!   Standard für alles, was nicht ausdrücklich freigegeben wurde.

use serde::{Deserialize, Serialize};

use crate::adapters::CapabilityRef;
use crate::core::{SchemaId, VersionId};

/// Schema-Vertrag der Betreiberregel.
pub const POLICY_SCHEMA: &str = "arcs.autonomy_policy.v1";
/// Schema-Vertrag der Autonomieentscheidung.
pub const DECISION_SCHEMA: &str = "arcs.autonomy_decision.v1";
/// Schema-Vertrag des Kostenbudgets.
pub const BUDGET_SCHEMA: &str = "arcs.autonomy_budget.v1";

/// Erzeuger autonomer Artefakte im Core.
pub const PUBLISHER: &str = "arcs.autonomy";

/// Obergrenze für Freitext in Betreiberregeln.
pub const MAX_POLICY_TEXT_BYTES: usize = 512;
/// Obergrenze für Fähigkeitsregeln je Betreiberregel.
pub const MAX_POLICY_CAPABILITIES: usize = 256;
/// Obergrenze für Trigger je Betreiberregel.
pub const MAX_POLICY_TRIGGERS: usize = 256;

/// Risikostufe einer vorgeschlagenen Wirkung.
///
/// Die Stufen sind geordnet. Der effektive Risikowert ist immer das Maximum
/// aus Betreiberregel und Vorschlag, damit ein Vorschlag sein eigenes Risiko
/// nicht herunterstufen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    /// Stabile Kurzbezeichnung für Protokolle und Berichte.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Umgebung, in der eine Wirkung stattfinden soll.
///
/// Die Umgebung ist Teil der Betreiberregel: Eine Fähigkeit, die im Test
/// freigegeben ist, ist damit nicht in der Produktion freigegeben.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Development,
    Test,
    Production,
}

impl Environment {
    /// Stabile Kurzbezeichnung für Protokolle und Berichte.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

/// Autonomiestufe einer vorgeschlagenen Wirkung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    /// Ausdrücklich erlaubte Routinearbeit.
    Routine,
    /// Bedingt autonom innerhalb festgelegter Grenzen.
    Bounded,
    /// Ein Mensch entscheidet.
    HumanDecision,
}

impl AutonomyLevel {
    /// Stabile Kurzbezeichnung für Protokolle und Berichte.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Routine => "routine",
            Self::Bounded => "bounded",
            Self::HumanDecision => "human_decision",
        }
    }
}

/// Grund einer Autonomieentscheidung.
///
/// Die Gründe sind stabil benannt, damit Berichte und Revisionen vergleichbar
/// bleiben. Freitext trägt nur die Begründung des Betreibers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AutonomyReason {
    /// Die Wirkung liegt innerhalb der Routinegrenzen.
    WithinRoutineBounds,
    /// Die Wirkung liegt innerhalb der bedingten Grenzen.
    WithinBoundedBounds,
    /// Die Fähigkeit ist in der Betreiberregel nicht bekannt.
    UnknownCapability,
    /// Die Umgebung ist für diese Fähigkeit nicht freigegeben.
    EnvironmentNotAllowed,
    /// Das Risiko liegt über der jeweiligen Stufe.
    RiskAboveBound,
    /// Der Preis liegt über der jeweiligen Stufe.
    CostAboveBound,
    /// Der Vorschlag nennt keinen belastbaren Preis.
    UnknownCost,
    /// Der Vorschlag nennt eine andere Währung als die Betreiberregel.
    CurrencyMismatch,
    /// Ein Auslöser der Betreiberregel verlangt eine menschliche Entscheidung.
    HumanTrigger { reason: String },
    /// Die Wirkung darf nach Betreiberregel nicht delegiert werden.
    NonDelegable { reason: String },
    /// Das Kostenbudget ist erschöpft.
    BudgetExhausted,
    /// Das Kostenbudget reicht für diese Wirkung nicht aus.
    BudgetExceeded,
    /// Für bedingte Autonomie ist kein Budget hinterlegt.
    NoBudgetConfigured,
}

/// Ergebnis der Einordnung einer vorgeschlagenen Wirkung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyAssessment {
    /// Die zugestellte Stufe.
    pub level: AutonomyLevel,
    /// Die nachvollziehbaren Gründe der Einordnung.
    pub reasons: Vec<AutonomyReason>,
}

/// Vorschlag eines Aufrufers über eine beabsichtigte Wirkung.
///
/// Der Vorschlag ist eine Behauptung. Er darf eine Stufe nie selbst
/// heraufstufen: Der Dienst führt Vorschlag und Betreiberregel zusammen und
/// entscheidet danach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionProfile {
    /// Die Fähigkeit, die die Wirkung ausüben soll.
    pub capability: CapabilityRef,
    /// Der Vertrag des Kandidaten, sofern bereits bekannt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<SchemaId>,
    /// Das behauptete Risiko der Wirkung.
    pub risk: RiskLevel,
    /// Die beabsichtigte Umgebung.
    pub environment: Environment,
    /// Der behauptete Preis in der kleinsten Einheit der Währung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<u64>,
    /// Die Währung des behaupteten Preises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Kurze Beschreibung der Wirkung.
    pub summary: String,
}

/// Betreiberregel für genau eine Fähigkeit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRule {
    /// Die Fähigkeit.
    pub adapter_id: String,
    /// Die Fähigkeit.
    pub capability_id: String,
    /// Optionale Einschränkung auf einen Kandidatenvertrag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,
    /// Das vom Betreiber veranschlagte Grundrisiko.
    pub baseline_risk: RiskLevel,
    /// Die Umgebungen, in denen die Fähigkeit wirken darf.
    pub environments: Vec<Environment>,
    /// Höchstzulässiger Preis je Wirkung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost: Option<u64>,
    /// Der vom Betreiber erwartete Preis.
    ///
    /// Er wirkt als Untergrenze: Ein Vorschlag kann den Preis nicht unter den
    /// Betreibererwartung deklarieren und so das Budgetbuch umgehen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_cost: Option<u64>,
    /// Die Währung, in der die Preisgrenzen gelten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// Grenzen einer Autonomiestufe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyBounds {
    /// Höchstzulässiges Risiko.
    pub max_risk: RiskLevel,
    /// Höchstzulässiger Preis je Wirkung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost: Option<u64>,
    /// Die zugelassenen Umgebungen. Eine leere Liste erlaubt keine Stufe.
    pub environments: Vec<Environment>,
}

/// Bedingung, die immer eine menschliche Entscheidung verlangt.
///
/// Alle gesetzten Felder müssen zutreffen. Ein leerer Auslöser trifft auf
/// jede Wirkung zu und ist damit die globale Bremse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTrigger {
    /// Optionale Einschränkung auf eine Fähigkeit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    /// Optionale Einschränkung auf eine Fähigkeit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    /// Optionale Einschränkung auf einen Kandidatenvertrag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,
    /// Optionale Risikoschwelle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_risk: Option<RiskLevel>,
    /// Optionale Preisschwelle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_cost: Option<u64>,
    /// Optionale Einschränkung auf eine Umgebung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Die Begründung des Betreibers.
    pub reason: String,
}

/// Regel, deren Wirkung nach Betreiberregel nicht delegierbar ist.
///
/// Sie bildet gesetzliche, vertragliche oder betreiberpolitische Grenzen ab,
/// an denen ARCS nicht selbst entscheiden darf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonDelegableRule {
    /// Optionale Einschränkung auf eine Fähigkeit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    /// Optionale Einschränkung auf eine Fähigkeit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    /// Optionale Einschränkung auf einen Kandidatenvertrag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,
    /// Optionale Einschränkung auf eine Umgebung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Die Begründung des Betreibers.
    pub reason: String,
}

/// Kumulatives Kostenbudget autonom erteilter Freigaben.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetPolicy {
    /// Stabile Kennung des Budgets.
    pub budget_id: String,
    /// Die Währung des Budgets.
    pub currency: String,
    /// Die Obergrenze in der kleinsten Einheit der Währung.
    pub limit: u64,
}

/// Die Betreiberregel, die Autonomie gewährt oder verweigert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyPolicy {
    /// Die Version der Regel.
    pub version: String,
    /// Die Regeln je Fähigkeit.
    pub capabilities: Vec<CapabilityRule>,
    /// Die Grenzen der Routinearbeit.
    pub routine: AutonomyBounds,
    /// Die Grenzen der bedingten Autonomie.
    pub bounded: AutonomyBounds,
    /// Auslöser für eine menschliche Entscheidung.
    pub human_triggers: Vec<HumanTrigger>,
    /// Grenzen, die nicht delegierbar sind.
    pub non_delegable: Vec<NonDelegableRule>,
    /// Das Kostenbudget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetPolicy>,
}

impl AutonomyPolicy {
    /// Prüft die strukturelle Gültigkeit der Betreiberregel.
    ///
    /// Die Prüfung schützt den Dienst vor leeren Regeln, die jede Wirkung
    /// freigeben würden, und vor Textfeldern ohne Grenze.
    pub fn validate(&self) -> Result<(), AutonomyError> {
        if self.version.trim().is_empty() || self.version.len() > MAX_POLICY_TEXT_BYTES {
            return Err(AutonomyError::Invalid("invalid policy version".into()));
        }
        if self.capabilities.len() > MAX_POLICY_CAPABILITIES {
            return Err(AutonomyError::Invalid("too many capability rules".into()));
        }
        if self.human_triggers.len() > MAX_POLICY_TRIGGERS {
            return Err(AutonomyError::Invalid("too many human triggers".into()));
        }
        if self.non_delegable.len() > MAX_POLICY_TRIGGERS {
            return Err(AutonomyError::Invalid(
                "too many non-delegable rules".into(),
            ));
        }
        for rule in &self.capabilities {
            if rule.adapter_id.trim().is_empty() || rule.capability_id.trim().is_empty() {
                return Err(AutonomyError::Invalid(
                    "a capability rule has no identity".into(),
                ));
            }
            if rule.environments.is_empty() {
                return Err(AutonomyError::Invalid(
                    "a capability rule without environments would allow every environment".into(),
                ));
            }
            if let Some(currency) = &rule.currency
                && currency.trim().is_empty()
            {
                return Err(AutonomyError::Invalid(
                    "a capability rule has no currency".into(),
                ));
            }
            if let (Some(maximum), Some(expected)) = (rule.max_cost, rule.expected_cost)
                && expected > maximum
            {
                return Err(AutonomyError::Invalid(
                    "the expected cost exceeds the allowed cost".into(),
                ));
            }
        }
        if self.routine.environments.is_empty() && self.bounded.environments.is_empty() {
            return Err(AutonomyError::Invalid(
                "a policy without any environment grants no autonomy".into(),
            ));
        }
        for trigger in &self.human_triggers {
            if trigger.reason.trim().is_empty() {
                return Err(AutonomyError::Invalid(
                    "a human trigger has no reason".into(),
                ));
            }
        }
        for rule in &self.non_delegable {
            if rule.reason.trim().is_empty() {
                return Err(AutonomyError::Invalid(
                    "a non-delegable rule has no reason".into(),
                ));
            }
        }
        if let Some(budget) = &self.budget
            && (budget.budget_id.trim().is_empty() || budget.currency.trim().is_empty())
        {
            return Err(AutonomyError::Invalid("the budget is incomplete".into()));
        }
        Ok(())
    }

    /// Die wirksame Fähigkeitsregel für einen Vorschlag.
    ///
    /// Bei mehreren passenden Regeln gewinnt die engere Regel; bei gleicher
    /// Enge gewinnt die konservativere. Damit bleibt die Auswahl eindeutig.
    pub(crate) fn matching_rule(&self, profile: &ActionProfile) -> Option<&CapabilityRule> {
        let mut best: Option<&CapabilityRule> = None;
        for rule in &self.capabilities {
            if rule.adapter_id != profile.capability.adapter_id.0
                || rule.capability_id != profile.capability.capability_id.0
            {
                continue;
            }
            if let Some(schema) = &rule.schema_id {
                let matches = profile
                    .schema_id
                    .as_ref()
                    .is_some_and(|candidate| candidate.0 == *schema);
                if !matches {
                    continue;
                }
            }
            let better = match best {
                None => true,
                Some(current) => {
                    let current_specific = current.schema_id.is_some();
                    let candidate_specific = rule.schema_id.is_some();
                    if candidate_specific && !current_specific {
                        true
                    } else if current_specific && !candidate_specific {
                        false
                    } else {
                        rule.baseline_risk > current.baseline_risk
                            || (rule.baseline_risk == current.baseline_risk
                                && rule.max_cost < current.max_cost)
                    }
                }
            };
            if better {
                best = Some(rule);
            }
        }
        best
    }
}

/// Die persistierte Sicht auf das Kostenbudget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyBudget {
    /// Stabile Kennung des Budgets.
    pub budget_id: String,
    /// Die Währung des Budgets.
    pub currency: String,
    /// Die Obergrenze.
    pub limit: u64,
    /// Die bereits gebuchten Kosten.
    pub spent: u64,
    /// Die verbleibenden Kosten.
    pub remaining: u64,
    /// Die Version der gespeicherten Sicht.
    pub version: u64,
    /// Die Version-ID der gespeicherten Sicht.
    pub version_id: VersionId,
}

/// Die persistierte Autonomieentscheidung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomyDecision {
    /// Stabile Kennung der Entscheidung.
    pub decision_id: String,
    /// Die Version der Betreiberregel.
    pub policy_version: String,
    /// Die Version-ID des Kandidaten.
    pub candidate_version: String,
    /// Die Fähigkeit der Wirkung.
    pub adapter_id: String,
    /// Die Fähigkeit der Wirkung.
    pub capability_id: String,
    /// Die zugestellte Stufe.
    pub level: AutonomyLevel,
    /// Die nachvollziehbaren Gründe.
    pub reasons: Vec<String>,
    /// Das wirksame Risiko.
    pub risk: RiskLevel,
    /// Die Umgebung.
    pub environment: Environment,
    /// Der wirksame Preis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<u64>,
    /// Die Währung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Die Version-ID der gespeicherten Entscheidung.
    pub version_id: VersionId,
}

/// Das Ergebnis einer autonom erteilten Freigabe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyAuthorization {
    /// Die zugestellte Stufe.
    pub level: AutonomyLevel,
    /// Die Version der aufgezeichneten Autonomieentscheidung.
    pub decision_version: VersionId,
    /// Die Version der Freigabe in der bestehenden Approval-Kette.
    pub approval_version: VersionId,
    /// Die im Budget gebuchten Kosten der Wirkung.
    pub reserved_cost: u64,
}

/// Fehler der kontrollierten Autonomie.
#[derive(Debug)]
pub enum AutonomyError {
    /// Die Anfrage ist strukturell unzulässig.
    Invalid(String),
    /// Der Store hat die Änderung nicht angenommen.
    Store(crate::store::StoreError),
    /// Das Schema ist nicht registriert.
    MissingSchema(String),
    /// Die Policy ist nicht gespeichert.
    MissingPolicy(VersionId),
    /// Die gespeicherte Policy hat einen fremden Vertrag.
    NotPolicyArtifact,
    /// Die Entscheidung ist nicht gespeichert.
    MissingDecision(VersionId),
    /// Die gespeicherte Entscheidung hat einen fremden Vertrag.
    NotDecisionArtifact,
    /// Der Kandidat fehlt.
    MissingCandidate(VersionId),
    /// Die Entscheidung verlangt einen Menschen.
    HumanDecisionRequired {
        /// Die Version der aufgezeichneten Entscheidung.
        decision_version: VersionId,
        /// Die Gründe der Eskalation.
        reasons: Vec<String>,
    },
    /// Das Budget wurde parallel verbraucht.
    BudgetRace {
        /// Die Kennung des Budgets.
        budget_id: String,
    },
    /// Das Budget reicht nicht aus.
    BudgetExceeded {
        /// Die Kennung des Budgets.
        budget_id: String,
        /// Die gebuchten Kosten.
        spent: u64,
        /// Die Obergrenze.
        limit: u64,
    },
    /// Die Freigabe selbst ist nicht möglich.
    Approval(crate::approval::ApprovalError),
}

impl From<crate::store::StoreError> for AutonomyError {
    fn from(value: crate::store::StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::approval::ApprovalError> for AutonomyError {
    fn from(value: crate::approval::ApprovalError) -> Self {
        Self::Approval(value)
    }
}

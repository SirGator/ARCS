//! Persistenz und Bewertung der kontrollierten Autonomie.
//!
//! Der Dienst beantwortet eine einzige Frage: Darf ARCS diese vorgeschlagene
//! Wirkung ohne menschliche Entscheidung auslösen? Die Antwort hängt
//! ausschließlich an der Betreiberregel und am Vorschlag, nie an der
//! Behauptung eines Modells über seine eigene Befugnis.
//!
//! Die Freigabe selbst bleibt in der bestehenden Kette: Eine autonome
//! Freigabe ist ein `arcs.approval.v1` mit einem System-Akteur, das über die
//! bestehende `ApprovalService`-Kette entsteht. Action und Execution müssen
//! dafür nicht angepasst werden.

use serde::{Deserialize, Serialize};

use crate::adapters::CapabilityRef;
use crate::approval::{ApprovalDecision, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, Provenance, SchemaId,
    SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, StoreError, relation_kinds};

use super::{
    ActionProfile, AutonomyAssessment, AutonomyAuthorization, AutonomyBudget, AutonomyDecision,
    AutonomyError, AutonomyLevel, AutonomyPolicy, AutonomyReason, BUDGET_SCHEMA, BudgetPolicy,
    DECISION_SCHEMA, POLICY_SCHEMA, PUBLISHER,
};

/// Subject des aktuell gültigen Betreiberregel-Artefakts.
pub const POLICY_SUBJECT: &str = "autonomy:policy";

/// Anfrage, eine vorgeschlagene Wirkung autonom freizugeben.
pub struct AutonomyRequest<'a> {
    /// Die Version der Betreiberregel, die angewendet wird.
    pub policy_version: &'a VersionId,
    /// Der Kandidat, der freigegeben werden soll.
    pub candidate_version: &'a VersionId,
    /// Der Prüfbericht, der den Kandidaten verifiziert.
    pub verification_report_version: &'a VersionId,
    /// Die vorgeschlagene Wirkung.
    pub profile: &'a ActionProfile,
}

/// Die wirksamen Werte einer vorgeschlagenen Wirkung.
///
/// Sie entstehen aus Betreiberregel und Vorschlag und sind damit konservativ:
/// Das Risiko ist das Maximum, der Preis das Maximum beider Angaben.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveProfile {
    risk: super::RiskLevel,
    cost: Option<u64>,
    currency: Option<String>,
}

/// Persistierter Zustand des Kostenbudgets.
#[derive(Debug, Deserialize, Serialize)]
struct BudgetPayload {
    budget_id: String,
    currency: String,
    limit: u64,
    spent: u64,
}

/// Persistierte Autonomieentscheidung.
#[derive(Debug, Deserialize, Serialize)]
struct DecisionPayload {
    decision_id: String,
    policy_version: String,
    candidate_version: String,
    capability: CapabilityPayload,
    level: AutonomyLevel,
    reasons: Vec<String>,
    risk: super::RiskLevel,
    environment: super::Environment,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    currency: Option<String>,
}

/// Fähigkeitsangabe innerhalb eines Artefakts.
#[derive(Debug, Deserialize, Serialize)]
struct CapabilityPayload {
    adapter_id: String,
    capability_id: String,
}

/// Dienst für die kontrollierte Autonomie.
pub struct AutonomyService<'a> {
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> AutonomyService<'a> {
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

    /// Legt die Betreiberregel als aktuell gültige Regel ab.
    ///
    /// Eine neue Regel ersetzt die bisherige im Current-State-Zeiger. Die
    /// Historie bleibt im Artefaktnetz erhalten, damit eine Freigabe immer
    /// auf die Regel verweisen kann, unter der sie erteilt wurde.
    pub fn publish_policy(&mut self, policy: &AutonomyPolicy) -> Result<Artifact, AutonomyError> {
        policy.validate()?;
        let subject = SubjectId(POLICY_SUBJECT.to_owned());
        let current = self
            .store
            .current(&subject, &SchemaId(POLICY_SCHEMA.into()))?;
        let version = current.as_ref().map_or(1, |artifact| artifact.version + 1);
        let payload = serde_json::to_value(policy)
            .map_err(|error| AutonomyError::Invalid(format!("policy: {error}")))?;
        let artifact = self.build(ArtifactDraft::derived(
            POLICY_SCHEMA,
            subject,
            payload,
            version,
            Vec::new(),
            vec![format!("policy:{}", policy.version)],
        ))?;
        self.store
            .append_current_resolved(&artifact, self.schemas)
            .map_err(AutonomyError::Store)
    }

    /// Liest die aktuell gültige Betreiberregel.
    pub fn current_policy(&self) -> Result<Option<Artifact>, AutonomyError> {
        Ok(self.store.current(
            &SubjectId(POLICY_SUBJECT.to_owned()),
            &SchemaId(POLICY_SCHEMA.into()),
        )?)
    }

    /// Liest eine gespeicherte Betreiberregel.
    pub fn policy(&self, version: &VersionId) -> Result<AutonomyPolicy, AutonomyError> {
        let artifact = self
            .store
            .get(version)?
            .ok_or_else(|| AutonomyError::MissingPolicy(version.clone()))?;
        if artifact.schema_id != SchemaId(POLICY_SCHEMA.into()) {
            return Err(AutonomyError::NotPolicyArtifact);
        }
        let policy: AutonomyPolicy = serde_json::from_value(artifact.payload.clone())
            .map_err(|error| AutonomyError::Invalid(format!("policy payload: {error}")))?;
        policy.validate()?;
        Ok(policy)
    }

    /// Liest eine aufgezeichnete Autonomieentscheidung.
    pub fn decision(&self, version: &VersionId) -> Result<AutonomyDecision, AutonomyError> {
        let artifact = self
            .store
            .get(version)?
            .ok_or_else(|| AutonomyError::MissingDecision(version.clone()))?;
        if artifact.schema_id != SchemaId(DECISION_SCHEMA.into()) {
            return Err(AutonomyError::NotDecisionArtifact);
        }
        let payload: DecisionPayload = serde_json::from_value(artifact.payload.clone())
            .map_err(|error| AutonomyError::Invalid(format!("decision payload: {error}")))?;
        Ok(AutonomyDecision {
            decision_id: payload.decision_id,
            policy_version: payload.policy_version,
            candidate_version: payload.candidate_version,
            adapter_id: payload.capability.adapter_id,
            capability_id: payload.capability.capability_id,
            level: payload.level,
            reasons: payload.reasons,
            risk: payload.risk,
            environment: payload.environment,
            cost: payload.cost,
            currency: payload.currency,
            version_id: artifact.version_id,
        })
    }

    /// Liest alle Entscheidungen zu einem Kandidaten unter einer Regel.
    ///
    /// Die Reihenfolge entspricht der Commit-Reihenfolge im Store und bleibt
    /// damit über einen Neustart hinweg nachvollziehbar.
    pub fn decisions(
        &self,
        candidate_version: &VersionId,
        policy_version: &VersionId,
    ) -> Result<Vec<AutonomyDecision>, AutonomyError> {
        let subject = SubjectId(format!(
            "autonomy:decision:autonomy:{}:{}",
            candidate_version.0, policy_version.0
        ));
        let artifacts = self
            .store
            .history(&subject, &SchemaId(DECISION_SCHEMA.into()))?;
        artifacts
            .iter()
            .map(|artifact| {
                let payload: DecisionPayload = serde_json::from_value(artifact.payload.clone())
                    .map_err(|error| {
                        AutonomyError::Invalid(format!("decision payload: {error}"))
                    })?;
                Ok(AutonomyDecision {
                    decision_id: payload.decision_id,
                    policy_version: payload.policy_version,
                    candidate_version: payload.candidate_version,
                    adapter_id: payload.capability.adapter_id,
                    capability_id: payload.capability.capability_id,
                    level: payload.level,
                    reasons: payload.reasons,
                    risk: payload.risk,
                    environment: payload.environment,
                    cost: payload.cost,
                    currency: payload.currency,
                    version_id: artifact.version_id.clone(),
                })
            })
            .collect()
    }

    /// Ordnet eine vorgeschlagene Wirkung einer Autonomiestufe zu.
    ///
    /// Die Einordnung ist eine reine Funktion aus Betreiberregel und
    /// Vorschlag. Sie schreibt nichts und kann deshalb für Vorschau,
    /// Bericht und Freigabe identisch verwendet werden.
    pub fn assess(policy: &AutonomyPolicy, profile: &ActionProfile) -> AutonomyAssessment {
        let Some(rule) = policy.matching_rule(profile) else {
            return AutonomyAssessment {
                level: AutonomyLevel::HumanDecision,
                reasons: vec![AutonomyReason::UnknownCapability],
            };
        };
        let effective = effective_profile(rule, profile);
        let mut blockers: Vec<AutonomyReason> = Vec::new();

        let Some(cost) = effective.cost else {
            blockers.push(AutonomyReason::UnknownCost);
            return AutonomyAssessment {
                level: AutonomyLevel::HumanDecision,
                reasons: blockers,
            };
        };

        if !currency_matches(rule, profile, policy, cost) {
            blockers.push(AutonomyReason::CurrencyMismatch);
        }
        if !rule.environments.contains(&profile.environment) {
            blockers.push(AutonomyReason::EnvironmentNotAllowed);
        }
        for restriction in &policy.non_delegable {
            if matches_restriction(restriction, profile) {
                blockers.push(AutonomyReason::NonDelegable {
                    reason: restriction.reason.clone(),
                });
            }
        }
        for trigger in &policy.human_triggers {
            if matches_trigger(trigger, profile, effective.risk, cost) {
                blockers.push(AutonomyReason::HumanTrigger {
                    reason: trigger.reason.clone(),
                });
            }
        }
        if !blockers.is_empty() {
            return AutonomyAssessment {
                level: AutonomyLevel::HumanDecision,
                reasons: blockers,
            };
        }

        if bounds_allow(&policy.routine, profile.environment, effective.risk, cost) {
            return AutonomyAssessment {
                level: AutonomyLevel::Routine,
                reasons: vec![AutonomyReason::WithinRoutineBounds],
            };
        }
        if bounds_allow(&policy.bounded, profile.environment, effective.risk, cost) {
            return AutonomyAssessment {
                level: AutonomyLevel::Bounded,
                reasons: vec![AutonomyReason::WithinBoundedBounds],
            };
        }

        let mut reasons = blockers;
        if effective.risk > policy.routine.max_risk && effective.risk > policy.bounded.max_risk {
            reasons.push(AutonomyReason::RiskAboveBound);
        }
        if !cost_within(cost, policy.routine.max_cost)
            && !cost_within(cost, policy.bounded.max_cost)
        {
            reasons.push(AutonomyReason::CostAboveBound);
        }
        if !policy.routine.environments.contains(&profile.environment)
            && !policy.bounded.environments.contains(&profile.environment)
        {
            reasons.push(AutonomyReason::EnvironmentNotAllowed);
        }
        AutonomyAssessment {
            level: AutonomyLevel::HumanDecision,
            reasons,
        }
    }

    /// Gibt eine vorgeschlagene Wirkung autonom frei.
    ///
    /// Der Ablauf ist durchgängig fail-closed: Eine unbekannte Fähigkeit, ein
    /// unbekannter Preis, eine nicht freigegebene Umgebung, ein ausgelöster
    /// Trigger oder ein erschöpftes Budget führen zu einer ausdrücklich
    /// protokollierten Entscheidung für einen Menschen. In keinem dieser
    /// Fälle entsteht eine Freigabe.
    ///
    /// Bei Erfolg entstehen zwei Artefakte: die Autonomieentscheidung und
    /// darüber die Freigabe in der bestehenden Approval-Kette.
    pub fn authorize(
        &mut self,
        request: &AutonomyRequest<'_>,
    ) -> Result<AutonomyAuthorization, AutonomyError> {
        let policy = self.policy(request.policy_version)?;
        let candidate = self
            .store
            .get(request.candidate_version)?
            .ok_or_else(|| AutonomyError::MissingCandidate(request.candidate_version.clone()))?;
        self.require_capability(&candidate, &request.profile.capability)?;
        if let Some(schema) = &request.profile.schema_id
            && candidate.schema_id != *schema
        {
            return Err(AutonomyError::Invalid(format!(
                "the candidate does not use the claimed contract '{}'",
                schema.0
            )));
        }

        let assessment = Self::assess(&policy, request.profile);
        let effective = effective_of(&policy, request.profile);
        if assessment.level == AutonomyLevel::HumanDecision {
            return Err(self.escalate(request, &assessment, &effective));
        }

        let reserved = effective.cost.unwrap_or(0);
        let mut reservation: Option<Reservation> = None;
        if reserved > 0 {
            match &policy.budget {
                None => {
                    // Bedingte Autonomie verlangt eine kumulative Budgetgrenze.
                    // Ohne sie bliebe nur der Preis je Wirkung, und die
                    // Summe der Wirkungen bliebe unbegrenzt.
                    if assessment.level == AutonomyLevel::Bounded {
                        let mut assessment = assessment;
                        assessment.level = AutonomyLevel::HumanDecision;
                        assessment.reasons.push(AutonomyReason::NoBudgetConfigured);
                        return Err(self.escalate(request, &assessment, &effective));
                    }
                }
                Some(budget) => {
                    reservation = Some(self.reserve_budget(
                        budget,
                        reserved,
                        request,
                        &assessment,
                        &effective,
                    )?);
                }
            }
        }

        let decision = match self.record_decision(request, &assessment, &effective) {
            Ok(decision) => decision,
            Err(error) => {
                // Ohne festgehaltene Entscheidung darf die gebuchte Kostenmenge
                // nicht ohne Freigabe im Budget liegen bleiben.
                if let Some(reservation) = &reservation {
                    let _ = self.release_budget(&reservation.budget_id, reservation.amount);
                }
                return Err(error);
            }
        };
        let reason = format!(
            "autonomy:{} under policy {} via decision {}",
            assessment.level.as_str(),
            request.policy_version.0,
            decision.version_id.0
        );
        let approval = {
            let mut approvals =
                ApprovalService::new(self.schemas, self.store, &mut *self.ids, self.clock);
            match approvals.decide_with_basis(
                request.candidate_version,
                request.verification_report_version,
                Some(&decision.version_id),
                ApprovalDecision::Approved,
                Actor {
                    actor_type: ActorType::System,
                    id: PUBLISHER.to_owned(),
                },
                reason,
            ) {
                Ok(approval) => approval,
                Err(error) => {
                    // Die Freigabe ist nicht zustande gekommen. Die gebuchte
                    // Kosten werden zurückgenommen, damit das Budget keine
                    // Wirkung ohne Freigabe belastet.
                    if let Some(reservation) = &reservation {
                        let _ = self.release_budget(&reservation.budget_id, reservation.amount);
                    }
                    return Err(AutonomyError::Approval(error));
                }
            }
        };

        Ok(AutonomyAuthorization {
            level: assessment.level,
            decision_version: decision.version_id,
            approval_version: approval.version_id,
            reserved_cost: reserved,
        })
    }

    /// Liest den aktuellen Stand des Kostenbudgets.
    pub fn budget(&self, budget_id: &str) -> Result<Option<AutonomyBudget>, AutonomyError> {
        let subject = SubjectId(budget_subject(budget_id));
        let Some(artifact) = self
            .store
            .current(&subject, &SchemaId(BUDGET_SCHEMA.into()))?
        else {
            return Ok(None);
        };
        let payload: BudgetPayload = serde_json::from_value(artifact.payload.clone())
            .map_err(|error| AutonomyError::Invalid(format!("budget payload: {error}")))?;
        Ok(Some(AutonomyBudget {
            budget_id: payload.budget_id,
            currency: payload.currency,
            limit: payload.limit,
            spent: payload.spent,
            remaining: payload.limit.saturating_sub(payload.spent),
            version: artifact.version,
            version_id: artifact.version_id,
        }))
    }

    /// Protokolliert eine Entscheidung für einen Menschen.
    ///
    /// Der Eintrag bleibt auch dann bestehen, wenn keine Freigabe entsteht.
    /// Er belegt, dass ARCS die Grenze erkannt hat und nicht eigenmächtig
    /// gehandelt hat.
    fn escalate(
        &mut self,
        request: &AutonomyRequest<'_>,
        assessment: &AutonomyAssessment,
        effective: &EffectiveProfile,
    ) -> AutonomyError {
        let reasons = reason_codes(&assessment.reasons);
        match self.record_decision(request, assessment, effective) {
            Ok(decision) => AutonomyError::HumanDecisionRequired {
                decision_version: decision.version_id,
                reasons,
            },
            Err(error) => error,
        }
    }

    /// Schreibt die Autonomieentscheidung als Core-Artefakt.
    fn record_decision(
        &mut self,
        request: &AutonomyRequest<'_>,
        assessment: &AutonomyAssessment,
        effective: &EffectiveProfile,
    ) -> Result<Artifact, AutonomyError> {
        let decision_id = format!(
            "autonomy:{}:{}",
            request.candidate_version.0, request.policy_version.0
        );
        let payload = DecisionPayload {
            decision_id: decision_id.clone(),
            policy_version: request.policy_version.0.clone(),
            candidate_version: request.candidate_version.0.clone(),
            capability: CapabilityPayload {
                adapter_id: request.profile.capability.adapter_id.0.clone(),
                capability_id: request.profile.capability.capability_id.0.clone(),
            },
            level: assessment.level,
            reasons: reason_codes(&assessment.reasons),
            risk: effective.risk,
            environment: request.profile.environment,
            cost: effective.cost,
            currency: effective.currency.clone(),
        };
        // Jede Freigabeentscheidung ist ein eigener Vorgang und erhält
        // deshalb eine eigene Identität. Die Historie bleibt im Netz.
        let generated = self.ids.next("autonomy_decision");
        let subject = SubjectId(format!("autonomy:decision:{decision_id}"));
        let parents = vec![
            request.policy_version.0.clone(),
            request.candidate_version.0.clone(),
        ];
        let artifact = self.build(ArtifactDraft {
            schema_id: DECISION_SCHEMA.to_owned(),
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            subject: subject.clone(),
            payload: serde_json::to_value(&payload)
                .map_err(|error| AutonomyError::Invalid(format!("decision: {error}")))?,
            version: 1,
            parents,
            tags: vec![
                format!("level:{}", assessment.level.as_str()),
                format!(
                    "capability:{}:{}",
                    payload.capability.adapter_id, payload.capability.capability_id
                ),
            ],
        })?;
        let relations = vec![
            (request.policy_version.clone(), relation_kinds::based_on()),
            (
                request.candidate_version.clone(),
                relation_kinds::based_on(),
            ),
        ];
        self.store
            .append_current_related_resolved(&artifact, self.schemas, &relations)
            .map_err(AutonomyError::Store)
    }

    /// Bucht Kosten im Budget und verhindert dabei eine Überschreitung.
    ///
    /// Die Buchung ist ein optimistischer Vergleich: Sie greift nur, wenn der
    /// gelesene Stand noch der aktuelle ist. Wer parallel gebucht hat,
    /// verliert den Wettlauf und erhält keine Freigabe.
    fn reserve_budget(
        &mut self,
        budget: &BudgetPolicy,
        amount: u64,
        request: &AutonomyRequest<'_>,
        assessment: &AutonomyAssessment,
        effective: &EffectiveProfile,
    ) -> Result<Reservation, AutonomyError> {
        let subject = SubjectId(budget_subject(&budget.budget_id));
        let schema = SchemaId(BUDGET_SCHEMA.into());
        let current = self.store.current(&subject, &schema)?;
        let (version, expected, spent, limit) = match &current {
            Some(artifact) => {
                let payload: BudgetPayload = serde_json::from_value(artifact.payload.clone())
                    .map_err(|error| AutonomyError::Invalid(format!("budget payload: {error}")))?;
                if payload.currency != budget.currency {
                    return Err(AutonomyError::Invalid(
                        "the stored budget uses another currency".into(),
                    ));
                }
                (
                    artifact.version + 1,
                    Some(artifact.version_id.clone()),
                    payload.spent,
                    payload.limit.min(budget.limit),
                )
            }
            None => (1, None, 0, budget.limit),
        };
        if spent.saturating_add(amount) > limit {
            // Die kumulative Grenze ist erreicht. Das ist eine Entscheidung
            // für einen Menschen und kein technischer Fehler.
            let mut reasons = assessment.reasons.clone();
            reasons.push(AutonomyReason::BudgetExceeded);
            let escalated = AutonomyAssessment {
                level: AutonomyLevel::HumanDecision,
                reasons,
            };
            return Err(self.escalate(request, &escalated, effective));
        }
        let payload = BudgetPayload {
            budget_id: budget.budget_id.clone(),
            currency: budget.currency.clone(),
            limit,
            spent: spent + amount,
        };
        let parents = current
            .as_ref()
            .map(|artifact| vec![artifact.version_id.0.clone()])
            .unwrap_or_default();
        let artifact = self.build(ArtifactDraft::derived(
            BUDGET_SCHEMA,
            subject.clone(),
            serde_json::to_value(&payload)
                .map_err(|error| AutonomyError::Invalid(format!("budget: {error}")))?,
            version,
            parents,
            vec![format!("budget:{}", budget.budget_id)],
        ))?;
        self.store
            .append_current_compared(&[artifact], self.schemas, &[(subject, schema, expected)])
            .map_err(|error| match error {
                StoreError::IdempotencyConflict => AutonomyError::BudgetRace {
                    budget_id: budget.budget_id.clone(),
                },
                other => AutonomyError::Store(other),
            })?;
        Ok(Reservation {
            budget_id: budget.budget_id.clone(),
            amount,
        })
    }

    /// Nimmt eine Kostenbuchung zurück, wenn die Freigabe ausblieb.
    fn release_budget(&mut self, budget_id: &str, amount: u64) -> Result<(), AutonomyError> {
        let subject = SubjectId(budget_subject(budget_id));
        let schema = SchemaId(BUDGET_SCHEMA.into());
        let Some(current) = self.store.current(&subject, &schema)? else {
            return Ok(());
        };
        let payload: BudgetPayload = serde_json::from_value(current.payload)
            .map_err(|error| AutonomyError::Invalid(format!("budget payload: {error}")))?;
        let spent = payload.spent.saturating_sub(amount);
        let artifact = self.build(ArtifactDraft::derived(
            BUDGET_SCHEMA,
            subject.clone(),
            serde_json::to_value(BudgetPayload {
                budget_id: payload.budget_id,
                currency: payload.currency,
                limit: payload.limit,
                spent,
            })
            .map_err(|error| AutonomyError::Invalid(format!("budget: {error}")))?,
            current.version + 1,
            vec![current.version_id.0.clone()],
            vec![format!("budget:{budget_id}")],
        ))?;
        // Schlägt die Rücknahme fehl, bleibt die Buchung stehen. Das ist
        // bewusst konservativ: Ein Budget darf nicht durch einen Fehler
        // kleiner erscheinen.
        self.store
            .append_current_compared(
                &[artifact],
                self.schemas,
                &[(subject, schema, Some(current.version_id))],
            )
            .map_err(AutonomyError::Store)?;
        Ok(())
    }

    /// Prüft, dass der Kandidat genau diese Fähigkeit verlangt.
    fn require_capability(
        &self,
        candidate: &Artifact,
        capability: &CapabilityRef,
    ) -> Result<(), AutonomyError> {
        let required = format!(
            "requires:{}:{}",
            capability.adapter_id.0, capability.capability_id.0
        );
        if !candidate.tags.iter().any(|tag| tag == &required) {
            return Err(AutonomyError::Invalid(format!(
                "the candidate does not require capability '{required}'"
            )));
        }
        Ok(())
    }

    /// Baut ein Core-Artefakt aus einem Entwurf.
    fn build(&mut self, draft: ArtifactDraft) -> Result<Artifact, AutonomyError> {
        let schema = self
            .schemas
            .get(&SchemaId(draft.schema_id.clone()))
            .cloned()
            .ok_or_else(|| AutonomyError::MissingSchema(draft.schema_id.clone()))?;
        Ok(Artifact {
            artifact_id: draft.artifact_id,
            version_id: draft.version_id,
            version: draft.version,
            artifact_type: schema.artifact_type.clone(),
            schema_id: schema.id.clone(),
            schema_version: schema.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.to_owned(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: draft.subject.0.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: draft.subject.0.clone(),
            subject: Some(draft.subject),
            tags: draft.tags,
            payload: draft.payload,
            provenance: Some(Provenance {
                parents: draft.parents,
                rules_applied: vec!["autonomy.decision".into()],
                models_used: vec![],
                transform: Some("autonomy.decision".into()),
            }),
            security: Default::default(),
        })
    }
}

/// Die Angaben zu einem neuen Core-Artefakt.
struct ArtifactDraft {
    schema_id: String,
    artifact_id: ArtifactId,
    version_id: VersionId,
    subject: SubjectId,
    payload: serde_json::Value,
    version: u64,
    parents: Vec<String>,
    tags: Vec<String>,
}

impl ArtifactDraft {
    /// Ein Entwurf, dessen Identität aus dem fachlichen Subject folgt.
    ///
    /// Nur so tragen die Fortschreibungen eines Budgets oder einer
    /// Betreiberregel eine lückenlose Versionsfolge. Der Store prüft die
    /// Versionsfolge je Artefaktidentität.
    fn derived(
        schema_id: &str,
        subject: SubjectId,
        payload: serde_json::Value,
        version: u64,
        parents: Vec<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            schema_id: schema_id.to_owned(),
            artifact_id: ArtifactId(subject.0.clone()),
            version_id: VersionId(format!("{}:v{version}", subject.0)),
            subject,
            payload,
            version,
            parents,
            tags,
        }
    }
}

/// Eine gebuchte Kostenreservierung.
struct Reservation {
    budget_id: String,
    amount: u64,
}

/// Führt Betreiberregel und Vorschlag zu den wirksamen Werten zusammen.
fn effective_profile(rule: &super::CapabilityRule, profile: &ActionProfile) -> EffectiveProfile {
    // Der wirksame Preis ist das Maximum aus Vorschlag und Betreibererwartung.
    // Ein Vorschlag kann den Preis damit nur zu niedrig, nie zu hoch
    // deklarieren. Ohne jede Angabe bleibt der Preis unbekannt.
    let cost = match (profile.cost, rule.expected_cost) {
        (Some(cost), expected) => Some(cost.max(expected.unwrap_or(0))),
        (None, Some(expected)) => Some(expected),
        (None, None) => None,
    };
    EffectiveProfile {
        risk: rule.baseline_risk.max(profile.risk),
        cost,
        currency: profile.currency.clone(),
    }
}

/// Die wirksamen Werte ohne auflösbare Fähigkeitsregel.
fn effective_of(policy: &AutonomyPolicy, profile: &ActionProfile) -> EffectiveProfile {
    match policy.matching_rule(profile) {
        Some(rule) => effective_profile(rule, profile),
        None => EffectiveProfile {
            risk: profile.risk,
            cost: profile.cost,
            currency: profile.currency.clone(),
        },
    }
}

/// Prüft die Währung gegen Fähigkeitsregel und Budget.
///
/// Ohne Preis ist die Währung bedeutungslos. Sonst muss sie mit der
/// Betreiberregel und dem Budget übereinstimmen. Eine andere Währung als
/// die des Budgets würde das Budgetbuch verfälschen.
fn currency_matches(
    rule: &super::CapabilityRule,
    profile: &ActionProfile,
    policy: &AutonomyPolicy,
    cost: u64,
) -> bool {
    if cost == 0 {
        return true;
    }
    if profile.currency.is_none() {
        return false;
    }
    if let Some(expected) = &rule.currency
        && profile.currency.as_deref() != Some(expected.as_str())
    {
        return false;
    }
    if let Some(budget) = &policy.budget
        && profile.currency.as_deref() != Some(budget.currency.as_str())
    {
        return false;
    }
    true
}

/// Prüft, ob eine Grenze eingehalten wird.
fn bounds_allow(
    bounds: &super::AutonomyBounds,
    environment: super::Environment,
    risk: super::RiskLevel,
    cost: u64,
) -> bool {
    bounds.environments.contains(&environment)
        && risk <= bounds.max_risk
        && cost_within(cost, bounds.max_cost)
}

/// Prüft einen Preis gegen eine optionale Obergrenze.
///
/// Eine Wirkung ohne Kosten braucht keine Preisgrenze. Kosten über null
/// brauchen eine ausdrücklich gesetzte Grenze; ohne sie bleibt die Stufe
/// abgelehnt.
fn cost_within(cost: u64, max_cost: Option<u64>) -> bool {
    if cost == 0 {
        return true;
    }
    max_cost.is_some_and(|maximum| cost <= maximum)
}

/// Prüft eine nicht delegierbare Regel.
fn matches_restriction(restriction: &super::NonDelegableRule, profile: &ActionProfile) -> bool {
    if let Some(adapter) = &restriction.adapter_id
        && adapter != &profile.capability.adapter_id.0
    {
        return false;
    }
    if let Some(capability) = &restriction.capability_id
        && capability != &profile.capability.capability_id.0
    {
        return false;
    }
    if let Some(schema) = &restriction.schema_id {
        let matches = profile
            .schema_id
            .as_ref()
            .is_some_and(|candidate| candidate.0 == *schema);
        if !matches {
            return false;
        }
    }
    if let Some(environment) = &restriction.environment
        && *environment != profile.environment
    {
        return false;
    }
    true
}

/// Prüft einen Auslöser für eine menschliche Entscheidung.
fn matches_trigger(
    trigger: &super::HumanTrigger,
    profile: &ActionProfile,
    risk: super::RiskLevel,
    cost: u64,
) -> bool {
    if let Some(adapter) = &trigger.adapter_id
        && adapter != &profile.capability.adapter_id.0
    {
        return false;
    }
    if let Some(capability) = &trigger.capability_id
        && capability != &profile.capability.capability_id.0
    {
        return false;
    }
    if let Some(schema) = &trigger.schema_id {
        let matches = profile
            .schema_id
            .as_ref()
            .is_some_and(|candidate| candidate.0 == *schema);
        if !matches {
            return false;
        }
    }
    if let Some(min_risk) = trigger.min_risk
        && risk < min_risk
    {
        return false;
    }
    if let Some(min_cost) = trigger.min_cost
        && cost < min_cost
    {
        return false;
    }
    if let Some(environment) = &trigger.environment
        && *environment != profile.environment
    {
        return false;
    }
    true
}

/// Stabile Kurzbezeichnungen der Gründe.
fn reason_codes(reasons: &[AutonomyReason]) -> Vec<String> {
    reasons
        .iter()
        .map(|reason| match reason {
            AutonomyReason::WithinRoutineBounds => "within_routine_bounds".to_owned(),
            AutonomyReason::WithinBoundedBounds => "within_bounded_bounds".to_owned(),
            AutonomyReason::UnknownCapability => "unknown_capability".to_owned(),
            AutonomyReason::EnvironmentNotAllowed => "environment_not_allowed".to_owned(),
            AutonomyReason::RiskAboveBound => "risk_above_bound".to_owned(),
            AutonomyReason::CostAboveBound => "cost_above_bound".to_owned(),
            AutonomyReason::UnknownCost => "unknown_cost".to_owned(),
            AutonomyReason::CurrencyMismatch => "currency_mismatch".to_owned(),
            AutonomyReason::HumanTrigger { reason } => format!("human_trigger:{reason}"),
            AutonomyReason::NonDelegable { reason } => format!("non_delegable:{reason}"),
            AutonomyReason::BudgetExhausted => "budget_exhausted".to_owned(),
            AutonomyReason::BudgetExceeded => "budget_exceeded".to_owned(),
            AutonomyReason::NoBudgetConfigured => "no_budget_configured".to_owned(),
        })
        .collect()
}

/// Subject des Budgets.
fn budget_subject(budget_id: &str) -> String {
    format!("autonomy:budget:{budget_id}")
}

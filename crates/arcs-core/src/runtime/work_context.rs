//! Persistente Arbeitskontexte als gemeinsame Betriebsobjekte der Runtime.
//!
//! Ein Arbeitskontext ist der leichtgewichtige, dauerhafte Zustand einer
//! Tätigkeit. Er referenziert Ziel, Task, Planrevision, Artefaktversionen,
//! Arbeitszustand, Authority-Scope, zulässige Memory-Sicht, Priorität, Budget
//! und laufende Operationen, ohne eigene Modellinstanzen zu starten.
//!
//! Die Trennung ist bewusst: Der Kontext dupliziert keine Fachinhalte. Er
//! hält Versionen und Zustände, damit Ergebnisse, Ereignisse und Freigaben
//! eindeutig zuzuordnen sind und ein Neustart den Zustand nicht verliert.
//!
//! Der Kontext ist selbst ein Core-Artefakt. Damit gelten für ihn dieselben
//! Regeln wie für jeden anderen Inhalt: kontrollierte Stufe, unveränderliche
//! Versionen, belegte Herkunft und ein Current-State-Zeiger je Tätigkeit.

mod records;
#[cfg(test)]
mod tests;

use crate::core::security::Classification;
use crate::core::{Clock, SchemaRegistry, VersionId};
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "arcs.work_context.v1";
pub const PUBLISHER: &str = "arcs.runtime.work_context";

/// Obergrenze für Freitextfelder eines Arbeitskontexts.
pub const MAX_CONTEXT_TEXT_BYTES: usize = 256;
/// Obergrenze für referenzierte Artefaktversionen je Kontext.
pub const MAX_CONTEXT_ARTIFACTS: usize = 256;
/// Obergrenze für gebundene Operationen je Kontext.
pub const MAX_CONTEXT_OPERATIONS: usize = 256;
/// Obergrenze für die Laufzeit eines Arbeitskontexts.
pub const MAX_BUDGET_WALL_CLOCK_MS: u64 = 86_400_000;
/// Obergrenze für verbrauchte Kontexttoken je Kontext.
pub const MAX_BUDGET_TOKENS: u64 = 100_000_000;
/// Obergrenze für Modellaufrufe je Kontext.
pub const MAX_BUDGET_MODEL_CALLS: u32 = 10_000;
/// Obergrenze für Kosten je Kontext.
pub const MAX_BUDGET_COST_MICROS: u64 = 1_000_000_000_000;

/// Arbeitszustand einer Tätigkeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkContextState {
    /// Angelegt, aber noch nicht begonnen.
    Pending,
    /// Aktiv in Bearbeitung.
    Running,
    /// Wartet auf eine äußere Bedingung; es läuft keine Modellinstanz.
    Waiting,
    /// Fachlich fertig, Freigabe steht noch aus.
    ReadyForReview,
    /// Abgebrochen mit Fehler.
    Failed,
    /// Erfolgreich abgeschlossen.
    Completed,
    /// Abgebrochen.
    Cancelled,
}

impl WorkContextState {
    /// Zustände, aus denen kein weiterer Übergang zulässig ist.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// Stabile Kurzbezeichnung für Berichte und Protokolle.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::ReadyForReview => "ready_for_review",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Grund, auf den eine Tätigkeit wartet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    /// Eine exklusive Ressource ist belegt.
    Resource,
    /// Eine Abhängigkeit ist noch nicht erfüllt.
    Dependency,
    /// Eine menschliche Freigabe steht aus.
    Approval,
    /// Ein Reasoner ist nicht verfügbar oder ausgelastet.
    Reasoning,
    /// Ein Adapter ist nicht verfügbar oder ausgelastet.
    Adapter,
    /// Eine externe Wirkung hat kein eindeutiges Ergebnis geliefert.
    External,
}

impl WaitKind {
    /// Stabile Kurzbezeichnung für Berichte und Protokolle.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Dependency => "dependency",
            Self::Approval => "approval",
            Self::Reasoning => "reasoning",
            Self::Adapter => "adapter",
            Self::External => "external",
        }
    }
}

/// Eine konkrete Wartebedingung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitCondition {
    pub kind: WaitKind,
    pub reference: String,
}

/// Zustand einer im Kontext gebundenen Operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Geplant, noch nicht begonnen.
    Planned,
    /// Aktiv.
    Running,
    /// Erfolgreich abgeschlossen.
    Succeeded,
    /// Abgebrochen mit Fehler.
    Failed,
    /// Abgebrochen.
    Cancelled,
    /// Das Ergebnis ist ungeklärt; die Wirkung wird nicht wiederholt.
    Unknown,
}

impl OperationState {
    /// Zustände, in denen eine Operation nicht mehr läuft.
    pub fn is_finished(self) -> bool {
        !matches!(self, Self::Planned | Self::Running)
    }

    /// Stabile Kurzbezeichnung für Berichte und Protokolle.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

/// Eine im Arbeitskontext gebundene Operation.
///
/// Die Operation bindet eine konkrete Ausführung an Kontext, Task und
/// Planrevision. Der Verweis auf das Ergebnis macht die Zuordnung eindeutig.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOperation {
    pub operation_id: String,
    pub kind: String,
    pub state: OperationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<VersionId>,
}

/// Authority-Scope einer Tätigkeit.
///
/// Der Scope beschreibt, was die Tätigkeit darf. Er begründet keine Authority
/// allein: Die wirksame Berechtigung ergibt sich immer aus dem Grant des
/// ausführenden Adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityScope {
    pub role_id: String,
    pub capabilities: Vec<String>,
    pub permissions: Vec<String>,
}

/// Zulässige Memory-Sicht einer Tätigkeit.
///
/// Die Sicht benennt die Verträge, aus denen gelesen werden darf, und die
/// stärkste Schutzstufe, die dabei offengelegt werden darf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryView {
    pub schema_ids: Vec<String>,
    pub max_classification: Classification,
}

/// Budget einer Tätigkeit.
///
/// Ein leeres Budget bedeutet „kein eigener Verbrauch"; die wirksame Grenze
/// entsteht weiterhin aus dem Grant des Adapters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkBudget {
    pub max_model_calls: u32,
    pub max_tokens: u64,
    pub max_cost_micros: u64,
    pub max_wall_clock_ms: u64,
}

/// Der persistierte Arbeitskontext einer Tätigkeit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkContext {
    pub context_id: String,
    pub runtime_id: String,
    pub task_sequence: u64,
    pub route_id: String,
    pub goal_id: Option<String>,
    pub plan_revision: Option<String>,
    pub state: WorkContextState,
    pub waiting_for: Vec<WaitCondition>,
    pub artifact_versions: Vec<VersionId>,
    pub authority: AuthorityScope,
    pub memory: MemoryView,
    pub priority: u8,
    pub budget: WorkBudget,
    pub operations: Vec<ContextOperation>,
    /// Version und Version-ID des gespeicherten Artefakts.
    pub version: u64,
    pub version_id: VersionId,
    /// Wirksame Schutzstufe des Kontexts.
    pub classification: Classification,
}

/// Entwurf eines neuen Arbeitskontexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkContextDraft {
    pub runtime_id: String,
    pub task_sequence: u64,
    pub route_id: String,
    pub goal_id: Option<String>,
    pub plan_revision: Option<String>,
    pub authority: AuthorityScope,
    pub memory: MemoryView,
    pub priority: u8,
    pub budget: WorkBudget,
    pub artifact_versions: Vec<VersionId>,
}

/// Eine kontrollierte Änderung an einem Arbeitskontext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextChange {
    /// Wechselt den Arbeitszustand und setzt die Wartebedingungen.
    State {
        state: WorkContextState,
        waiting_for: Vec<WaitCondition>,
        reason: String,
    },
    /// Bindet eine weitere Artefaktversion an den Kontext.
    Artifact { version: VersionId },
    /// Bindet eine Operation oder aktualisiert ihren Zustand.
    Operation { operation: ContextOperation },
    /// Setzt Ziel und Planrevision der Tätigkeit.
    Plan {
        goal_id: Option<String>,
        plan_revision: Option<String>,
    },
    /// Setzt Priorität und Budget der Tätigkeit.
    Resources { priority: u8, budget: WorkBudget },
}

/// Fehler der Arbeitskontext-Grenze.
#[derive(Debug)]
pub enum WorkContextError {
    /// Der Store hat die Änderung nicht angenommen.
    Store(StoreError),
    /// Die Anfrage ist strukturell unzulässig.
    Invalid(String),
    /// Der Zustandswechsel ist nicht zulässig.
    Transition(String),
    /// Eine referenzierte Artefaktversion existiert nicht.
    UnknownArtifact(VersionId),
    /// Für diese Tätigkeit existiert bereits ein abweichender Kontext.
    AlreadyOpen(String),
}

impl From<StoreError> for WorkContextError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

fn invalid(message: impl Into<String>) -> WorkContextError {
    WorkContextError::Invalid(message.into())
}

/// Dienst für die persistenten Arbeitskontexte einer Runtime.
///
/// Der Dienst kennt keine Fachlogik. Er sichert lediglich Zustandsübergänge,
/// Referenzen und Bindungen und schreibt sie als Core-Artefakte fort.
pub struct WorkContextService<'a> {
    store: &'a SqliteArtifactStore,
    schemas: &'a SchemaRegistry,
    clock: &'a dyn Clock,
}

impl<'a> WorkContextService<'a> {
    pub fn new(
        store: &'a SqliteArtifactStore,
        schemas: &'a SchemaRegistry,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            store,
            schemas,
            clock,
        }
    }

    /// Öffnet den Arbeitskontext einer Tätigkeit.
    ///
    /// Der Aufruf ist idempotent: Existiert bereits ein Kontext für dieselbe
    /// Tätigkeit, wird er unverändert zurückgegeben. Das macht einen Neustart
    /// nach einem Absturz sicher, ohne doppelte Zustände zu erzeugen. Weicht der
    /// bestehende Kontext in seiner Identität ab, wird der Aufruf abgelehnt.
    pub fn open(&self, draft: &WorkContextDraft) -> Result<WorkContext, WorkContextError> {
        self.validate_draft(draft)?;
        if let Some(existing) = self.current(&draft.runtime_id, draft.task_sequence)? {
            if existing.route_id != draft.route_id {
                return Err(WorkContextError::AlreadyOpen(format!(
                    "task {} is already bound to route '{}'",
                    draft.task_sequence, existing.route_id
                )));
            }
            return Ok(existing);
        }
        let classification = self.reference_classification(&draft.artifact_versions, None)?;
        let payload = WorkContextPayload {
            context_id: records::context_id(&draft.runtime_id, draft.task_sequence),
            runtime_id: draft.runtime_id.clone(),
            task_sequence: draft.task_sequence,
            route_id: draft.route_id.clone(),
            goal_id: draft.goal_id.clone(),
            plan_revision: draft.plan_revision.clone(),
            state: WorkContextState::Pending,
            waiting_for: Vec::new(),
            artifact_versions: draft.artifact_versions.clone(),
            authority: draft.authority.clone(),
            memory: draft.memory.clone(),
            priority: draft.priority,
            budget: draft.budget,
            operations: Vec::new(),
        };
        let artifact = records::make(&payload, 1, None, classification, self.clock, self.schemas)?;
        let stored = self
            .store
            .append_current_resolved(&artifact, self.schemas)?;
        records::parse(&stored)
    }

    /// Schreibt eine kontrollierte Fortschreibung des Arbeitskontexts.
    ///
    /// Der Übergang ist an den gespeicherten Zustand gebunden: Ein veralteter
    /// Stand wird mit einem Versionskonflikt abgelehnt, statt eine fremde
    /// Fortschreibung zu überschreiben. Ohne tatsächliche Änderung wird kein
    /// neues Artefakt geschrieben.
    pub fn advance(
        &self,
        current: &WorkContext,
        changes: &[ContextChange],
    ) -> Result<WorkContext, WorkContextError> {
        if changes.is_empty() {
            return Ok(current.clone());
        }
        let mut payload = records::payload(current)?;
        let mut touched = false;
        for change in changes {
            touched |= apply_change(&mut payload, change)?;
        }
        if !touched {
            return Ok(current.clone());
        }
        validate_payload(&payload, self.schemas)?;
        let classification = self
            .reference_classification(&payload.artifact_versions, Some(current.classification))?;
        let version = current.version + 1;
        let artifact = records::make(
            &payload,
            version,
            Some(&current.version_id),
            classification,
            self.clock,
            self.schemas,
        )?;
        let stored = self
            .store
            .append_current_resolved(&artifact, self.schemas)?;
        records::parse(&stored)
    }

    /// Liest den aktuellen Arbeitskontext einer Tätigkeit.
    pub fn current(
        &self,
        runtime_id: &str,
        task_sequence: u64,
    ) -> Result<Option<WorkContext>, WorkContextError> {
        let Some(artifact) = self.store.current(
            &records::subject(runtime_id, task_sequence),
            &crate::core::SchemaId(SCHEMA.into()),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(records::parse(&artifact)?))
    }

    /// Liest die vollständige Historie eines Arbeitskontexts.
    pub fn history(
        &self,
        runtime_id: &str,
        task_sequence: u64,
    ) -> Result<Vec<WorkContext>, WorkContextError> {
        let artifacts = self.store.history(
            &records::subject(runtime_id, task_sequence),
            &crate::core::SchemaId(SCHEMA.into()),
        )?;
        artifacts.iter().map(records::parse).collect()
    }

    /// Bindet eine Operation an den Kontext und schreibt das Ergebnis fest.
    ///
    /// Diese Fassade prüft zusätzlich, dass das Ergebnis zum selben Task
    /// gehört: Eine fremde Task-Zuordnung wird abgelehnt.
    pub fn bind_operation(
        &self,
        current: &WorkContext,
        operation: ContextOperation,
        result: Option<&VersionId>,
    ) -> Result<WorkContext, WorkContextError> {
        let mut operation = operation;
        if operation.version_id.is_none() {
            operation.version_id = result.cloned();
        }
        if operation.version_id.is_some()
            && result.is_some()
            && operation.version_id.as_ref() != result
        {
            return Err(invalid("operation result differs from the bound version"));
        }
        self.advance(current, &[ContextChange::Operation { operation }])
    }

    fn validate_draft(&self, draft: &WorkContextDraft) -> Result<(), WorkContextError> {
        check_text("runtime_id", &draft.runtime_id)?;
        check_text("route_id", &draft.route_id)?;
        if draft.task_sequence == 0 {
            return Err(invalid("task_sequence must be at least 1"));
        }
        if let Some(goal_id) = &draft.goal_id {
            check_text("goal_id", goal_id)?;
        }
        if let Some(plan_revision) = &draft.plan_revision {
            check_text("plan_revision", plan_revision)?;
        }
        if draft.priority > 3 {
            return Err(invalid("priority must be within 0..=3"));
        }
        check_text("authority.role_id", &draft.authority.role_id)?;
        for capability in &draft.authority.capabilities {
            check_text("authority.capabilities", capability)?;
        }
        for permission in &draft.authority.permissions {
            check_text("authority.permissions", permission)?;
        }
        for schema_id in &draft.memory.schema_ids {
            if !self
                .schemas
                .get(&crate::core::SchemaId(schema_id.clone()))
                .is_some()
            {
                return Err(invalid(format!(
                    "memory view names an unregistered schema '{schema_id}'"
                )));
            }
        }
        check_budget(&draft.budget)?;
        if draft.artifact_versions.len() > MAX_CONTEXT_ARTIFACTS {
            return Err(invalid("too many referenced artifacts"));
        }
        Ok(())
    }

    /// Ermittelt die stärkste Schutzstufe der referenzierten Artefakte.
    ///
    /// Ein Kontext, der auf vertrauliche Inhalte verweist, ist selbst nicht
    /// weniger schutzbedürftig. Ohne Referenz bleibt die Standardstufe.
    fn reference_classification(
        &self,
        versions: &[VersionId],
        previous: Option<Classification>,
    ) -> Result<Classification, WorkContextError> {
        let mut effective = previous.unwrap_or(Classification::DEFAULT);
        for version in versions {
            let artifact = self
                .store
                .get(version)?
                .ok_or_else(|| WorkContextError::UnknownArtifact(version.clone()))?;
            effective = effective.strongest(artifact.security.effective());
        }
        Ok(effective)
    }
}

/// Prüft einen Freitextfeld des Arbeitskontexts.
fn check_text(field: &str, value: &str) -> Result<(), WorkContextError> {
    if value.trim().is_empty()
        || value.len() > MAX_CONTEXT_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(invalid(format!(
            "{field} is empty, too long, or has control characters"
        )));
    }
    Ok(())
}

/// Prüft die Grenzen eines Budgets.
fn check_budget(budget: &WorkBudget) -> Result<(), WorkContextError> {
    if budget.max_model_calls > MAX_BUDGET_MODEL_CALLS
        || budget.max_tokens > MAX_BUDGET_TOKENS
        || budget.max_cost_micros > MAX_BUDGET_COST_MICROS
        || budget.max_wall_clock_ms > MAX_BUDGET_WALL_CLOCK_MS
    {
        return Err(invalid("budget exceeds the allowed limits"));
    }
    Ok(())
}

/// Wendet eine Änderung auf den Payload an und meldet, ob sich etwas änderte.
fn apply_change(
    payload: &mut WorkContextPayload,
    change: &ContextChange,
) -> Result<bool, WorkContextError> {
    match change {
        ContextChange::State {
            state,
            waiting_for,
            reason,
        } => {
            check_text("reason", reason)?;
            if !allowed_transition(payload.state, *state) {
                return Err(WorkContextError::Transition(format!(
                    "cannot move from {:?} to {:?}",
                    payload.state, state
                )));
            }
            if *state == WorkContextState::Waiting && waiting_for.is_empty() {
                return Err(invalid("a waiting context needs a wait condition"));
            }
            if *state != WorkContextState::Waiting && !waiting_for.is_empty() {
                return Err(invalid("only a waiting context may carry wait conditions"));
            }
            for condition in waiting_for {
                check_text("waiting_for.reference", &condition.reference)?;
            }
            if *state == WorkContextState::Completed
                && payload
                    .operations
                    .iter()
                    .any(|operation| !operation.state.is_finished())
            {
                return Err(WorkContextError::Transition(
                    "a context with unfinished operations cannot be completed".into(),
                ));
            }
            let changed = payload.state != *state || payload.waiting_for != *waiting_for;
            payload.state = *state;
            payload.waiting_for = waiting_for.clone();
            Ok(changed)
        }
        ContextChange::Artifact { version } => {
            if payload.artifact_versions.contains(version) {
                return Ok(false);
            }
            if payload.artifact_versions.len() >= MAX_CONTEXT_ARTIFACTS {
                return Err(invalid("too many referenced artifacts"));
            }
            payload.artifact_versions.push(version.clone());
            Ok(true)
        }
        ContextChange::Operation { operation } => {
            check_text("operation.operation_id", &operation.operation_id)?;
            check_text("operation.kind", &operation.kind)?;
            if let Some(version) = &operation.version_id {
                check_text("operation.version_id", &version.0)?;
            }
            match payload
                .operations
                .iter_mut()
                .find(|existing| existing.operation_id == operation.operation_id)
            {
                Some(existing) => {
                    if existing.state == operation.state {
                        // Erneutes Binden desselben Zustands ist idempotent.
                        // Es darf aber kein anderes Ergebnis vortaeuschen.
                        if existing.version_id.is_some()
                            && operation.version_id.is_some()
                            && existing.version_id != operation.version_id
                        {
                            return Err(WorkContextError::Transition(format!(
                                "operation '{}' was already bound to a different result",
                                operation.operation_id
                            )));
                        }
                    } else if !allowed_operation_transition(existing.state, operation.state) {
                        return Err(WorkContextError::Transition(format!(
                            "operation '{}' cannot move from {:?} to {:?}",
                            operation.operation_id, existing.state, operation.state
                        )));
                    }
                    let changed = existing.state != operation.state
                        || existing.version_id != operation.version_id
                        || existing.kind != operation.kind;
                    existing.state = operation.state;
                    existing.kind = operation.kind.clone();
                    existing.version_id = operation.version_id.clone();
                    Ok(changed)
                }
                None => {
                    if payload.operations.len() >= MAX_CONTEXT_OPERATIONS {
                        return Err(invalid("too many bound operations"));
                    }
                    payload.operations.push(operation.clone());
                    Ok(true)
                }
            }
        }
        ContextChange::Plan {
            goal_id,
            plan_revision,
        } => {
            if let Some(goal_id) = goal_id {
                check_text("goal_id", goal_id)?;
            }
            if let Some(plan_revision) = plan_revision {
                check_text("plan_revision", plan_revision)?;
            }
            let changed = payload.goal_id != *goal_id || payload.plan_revision != *plan_revision;
            payload.goal_id = goal_id.clone();
            payload.plan_revision = plan_revision.clone();
            Ok(changed)
        }
        ContextChange::Resources { priority, budget } => {
            if *priority > 3 {
                return Err(invalid("priority must be within 0..=3"));
            }
            check_budget(budget)?;
            let changed = payload.priority != *priority || payload.budget != *budget;
            payload.priority = *priority;
            payload.budget = *budget;
            Ok(changed)
        }
    }
}

/// Zulässige Zustandsübergänge eines Arbeitskontexts.
///
/// Terminale Zustände bleiben terminal. `Failed` darf erneut gestartet
/// werden, weil ein Wiederholversuch eine eigene Operation bindet.
fn allowed_transition(from: WorkContextState, to: WorkContextState) -> bool {
    use WorkContextState::{
        Cancelled, Completed, Failed, Pending, ReadyForReview, Running, Waiting,
    };
    if from == to {
        return true;
    }
    match (from, to) {
        (Pending, Running | Cancelled | Failed) => true,
        (Running, Waiting | ReadyForReview | Failed | Completed | Cancelled | Running) => true,
        (Waiting, Running | ReadyForReview | Completed | Failed | Cancelled) => true,
        (ReadyForReview, Completed | Failed | Cancelled | Running) => true,
        (Failed, Running | Cancelled) => true,
        (Completed | Cancelled, _) => false,
        _ => false,
    }
}

/// Zulässige Zustandsübergänge einer gebundenen Operation.
///
/// `Unknown` darf in jeden Ausgangszustand überführt werden: Das ist die
/// ausdrückliche Reconciliation eines unklaren externen Ergebnisses.
fn allowed_operation_transition(from: OperationState, to: OperationState) -> bool {
    use OperationState::{Cancelled, Failed, Planned, Running, Succeeded, Unknown};
    match (from, to) {
        (Planned, Running | Failed | Cancelled) => true,
        (Running, Succeeded | Failed | Cancelled | Unknown) => true,
        (Unknown, Running | Succeeded | Failed | Cancelled) => true,
        (Failed, Running) => true,
        (Succeeded | Cancelled, _) => false,
        _ => from == to,
    }
}

/// Der serialisierbare Payload eines Arbeitskontexts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkContextPayload {
    pub context_id: String,
    pub runtime_id: String,
    pub task_sequence: u64,
    pub route_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_revision: Option<String>,
    pub state: WorkContextState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_for: Vec<WaitCondition>,
    pub artifact_versions: Vec<VersionId>,
    pub authority: AuthorityScope,
    pub memory: MemoryView,
    pub priority: u8,
    pub budget: WorkBudget,
    pub operations: Vec<ContextOperation>,
}

/// Prüft den Payload eines Arbeitskontexts gegen seine eigenen Grenzen.
fn validate_payload(
    payload: &WorkContextPayload,
    schemas: &SchemaRegistry,
) -> Result<(), WorkContextError> {
    check_text("context_id", &payload.context_id)?;
    check_text("runtime_id", &payload.runtime_id)?;
    check_text("route_id", &payload.route_id)?;
    if payload.task_sequence == 0 {
        return Err(invalid("task_sequence must be at least 1"));
    }
    if payload.priority > 3 {
        return Err(invalid("priority must be within 0..=3"));
    }
    check_budget(&payload.budget)?;
    if payload.artifact_versions.len() > MAX_CONTEXT_ARTIFACTS {
        return Err(invalid("too many referenced artifacts"));
    }
    if payload.operations.len() > MAX_CONTEXT_OPERATIONS {
        return Err(invalid("too many bound operations"));
    }
    let mut seen = BTreeSet::new();
    for version in &payload.artifact_versions {
        if !seen.insert(version.0.clone()) {
            return Err(invalid("duplicate artifact reference"));
        }
    }
    if schemas.get(&crate::core::SchemaId(SCHEMA.into())).is_none() {
        return Err(invalid("missing work context schema"));
    }
    Ok(())
}

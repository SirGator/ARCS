//! Anbindung der Aufgabenruntime an die persistenten Arbeitskontexte.
//!
//! Die Runtime startet für eine Aufgabe keine eigene Agenteninstanz. Sie führt
//! stattdessen einen leichtgewichtigen Arbeitskontext im Core, der Ziel, Task,
//! Planrevision, Artefaktversionen, Authority-Scope, Memory-Sicht, Priorität,
//! Budget und die laufenden Operationen bindet.
//!
//! Der Kontext ist damit die einzige Stelle, an der ein Ergebnis, ein Ereignis
//! oder eine Freigabe einer Aufgabe zugeordnet wird. Er ersetzt weder das
//! Journal noch den Invocation-Store, sondern verweist auf beide.

use arcs::core::security::Classification;
use arcs::core::{Artifact, SystemClock, VersionId};
use arcs::runtime::work_context::{
    AuthorityScope, ContextChange, ContextOperation, MemoryView, OperationState, WorkBudget,
    WorkContext, WorkContextDraft, WorkContextError, WorkContextService, WorkContextState,
};
use serde::Serialize;

use super::{HostError, ReasoningRoute, TaskRuntime};
use crate::Host;

/// Gemeinsame Uhr der Arbeitskontexte. Der Core braucht keine eigene
/// Zeitquelle pro Tätigkeit.
static CLOCK: SystemClock = SystemClock;

/// Sichtbarer Zustand eines Arbeitskontexts für autorisierte Statusabfragen.
///
/// Der Bericht enthält nur Zustand und Bindungen, keine Nutzdaten. Damit
/// bleibt eine Statusabfrage auch für weniger privilegierte Betrachter
/// unkritisch.
#[derive(Debug, Clone, Serialize)]
pub struct WorkContextReport {
    pub context_id: String,
    pub task_sequence: u64,
    pub route_id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_revision: Option<String>,
    pub priority: u8,
    pub artifact_versions: usize,
    pub operations: Vec<OperationReport>,
    pub waiting_for: Vec<String>,
    pub classification: Classification,
}

/// Sichtbare Bindung einer Operation.
#[derive(Debug, Clone, Serialize)]
pub struct OperationReport {
    pub operation_id: String,
    pub kind: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

impl TaskRuntime {
    fn context_service<'a>(&self, host: &'a Host) -> WorkContextService<'a> {
        WorkContextService::new(host.store(), host.schemas(), &CLOCK)
    }

    /// Baut den Entwurf eines Arbeitskontexts aus der gespeicherten Route.
    ///
    /// Der Entwurf beschreibt nur, was die Aufgabe darf und woran sie arbeitet.
    /// Die wirksame Berechtigung ergibt sich weiterhin aus dem Grant des
    /// ausführenden Adapters.
    fn context_draft(
        &self,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
        goal_id: Option<String>,
        plan_revision: Option<String>,
    ) -> WorkContextDraft {
        let mut capabilities = vec![capability_key(&route.reasoner)];
        for capability in &route.allowed_capabilities {
            let key = capability_key(capability);
            if !capabilities.contains(&key) {
                capabilities.push(key);
            }
        }
        let max_classification = route
            .memory
            .as_ref()
            .and_then(|policy| {
                policy
                    .scopes
                    .iter()
                    .filter_map(|scope| scope.filter.max_classification)
                    .max_by_key(|classification| classification.rank())
            })
            .unwrap_or(Classification::Internal);
        WorkContextDraft {
            runtime_id: self.config.runtime_id.clone(),
            task_sequence: sequence,
            route_id: route.id.clone(),
            goal_id,
            plan_revision,
            authority: AuthorityScope {
                role_id: route.id.clone(),
                capabilities,
                permissions: Vec::new(),
            },
            memory: MemoryView {
                schema_ids: vec![route.input_schema_id.0.clone()],
                max_classification,
            },
            priority: route.priority,
            budget: WorkBudget {
                max_model_calls: route.max_reasoning_requests.unwrap_or(0),
                max_tokens: route
                    .budget
                    .max_output_tokens
                    .map(|tokens| tokens as u64)
                    .unwrap_or(route.budget.max_context_bytes as u64)
                    .min(arcs::runtime::work_context::MAX_BUDGET_TOKENS),
                max_cost_micros: 0,
                max_wall_clock_ms: route.budget.max_runtime_ms,
            },
            artifact_versions: vec![input.version_id.clone()],
        }
    }

    /// Öffnet den Arbeitskontext einer Aufgabe und bringt ihn in den Laufzustand.
    ///
    /// Der Aufruf ist idempotent. Ein bereits bekannter Kontext wird
    /// unverändert weiterverwendet; ein fremder Kontext für dieselbe Aufgabe
    /// wird abgelehnt.
    pub(super) fn start_work_context(
        &self,
        host: &Host,
        sequence: u64,
        route: &ReasoningRoute,
        input: &Artifact,
    ) -> Result<WorkContext, HostError> {
        let goal_id = self
            .saved_goal(sequence)?
            .map(|goal| format!("goal:{}", goal.sequence));
        let plan_revision = self.journal.plan_revision(sequence)?;
        let draft = self.context_draft(sequence, route, input, goal_id, plan_revision);
        let service = self.context_service(host);
        let mut context = service.open(&draft).map_err(HostError::WorkContext)?;
        // Ziel und Planrevision können sich während der Aufgabe ändern.
        // Die Abweichung wird als eigener Übergang festgehalten.
        if context.goal_id != draft.goal_id || context.plan_revision != draft.plan_revision {
            context = service
                .advance(
                    &context,
                    &[ContextChange::Plan {
                        goal_id: draft.goal_id.clone(),
                        plan_revision: draft.plan_revision.clone(),
                    }],
                )
                .map_err(HostError::WorkContext)?;
        }
        if context.state == WorkContextState::Pending {
            context = service
                .advance(
                    &context,
                    &[ContextChange::State {
                        state: WorkContextState::Running,
                        waiting_for: Vec::new(),
                        reason: "task dispatched".into(),
                    }],
                )
                .map_err(HostError::WorkContext)?;
        }
        Ok(context)
    }

    /// Bindet das Ergebnis einer Aufgabe an ihren Arbeitskontext.
    ///
    /// Das Ergebnis wird als Operation gebunden und der Kontext in die
    /// Freigabe geführt. Der Context wird vor dem Übergang neu gelesen,
    /// damit ein zwischenzeitlich geschriebener Stand nicht überschrieben
    /// wird.
    pub(super) fn finish_work_context(
        &self,
        host: &Host,
        sequence: u64,
        result_version: &VersionId,
    ) -> Result<(), HostError> {
        let Some(current) = self.current_work_context(host, sequence)? else {
            return Ok(());
        };
        // Erneutes Binden derselben Operation ist idempotent. Ein bereits an
        // ein anderes Ergebnis gebundenes Ergebnis bleibt aber ein Konflikt,
        // der den Task nicht abbrechen darf.
        let current = match self.bind_work_context_operation(
            host,
            &current,
            &format!("reasoning:{sequence}"),
            "reasoning",
            OperationState::Succeeded,
            Some(result_version),
        ) {
            Ok(current) => current,
            Err(HostError::WorkContext(WorkContextError::Transition(_))) => current,
            Err(error) => return Err(error),
        };
        match self.advance_work_context(
            host,
            &current,
            &[ContextChange::State {
                state: WorkContextState::ReadyForReview,
                waiting_for: Vec::new(),
                reason: "task result ready for review".into(),
            }],
        ) {
            Ok(_) => Ok(()),
            // Das Journal bleibt führ den Aufgabenzustand maßgeblich. Ein
            // unzulässiger Übergang im Kontext darf den Task nicht
            // abbrechen; ein Speicherfehler dagegen schon.
            Err(HostError::WorkContext(WorkContextError::Transition(_))) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Liest den gespeicherten Arbeitskontext einer Aufgabe.
    pub(super) fn current_work_context(
        &self,
        host: &Host,
        sequence: u64,
    ) -> Result<Option<WorkContext>, HostError> {
        self.context_service(host)
            .current(&self.config.runtime_id, sequence)
            .map_err(HostError::WorkContext)
    }

    /// Schreibt Zustandsübergänge des Arbeitskontexts.
    pub(super) fn advance_work_context(
        &self,
        host: &Host,
        context: &WorkContext,
        changes: &[ContextChange],
    ) -> Result<WorkContext, HostError> {
        self.context_service(host)
            .advance(context, changes)
            .map_err(HostError::WorkContext)
    }

    /// Bindet eine Operation und ihr Ergebnis an den Arbeitskontext.
    pub(super) fn bind_work_context_operation(
        &self,
        host: &Host,
        context: &WorkContext,
        operation_id: &str,
        kind: &str,
        state: OperationState,
        result: Option<&VersionId>,
    ) -> Result<WorkContext, HostError> {
        self.context_service(host)
            .bind_operation(
                context,
                ContextOperation {
                    operation_id: operation_id.to_owned(),
                    kind: kind.to_owned(),
                    state,
                    version_id: None,
                },
                result,
            )
            .map_err(HostError::WorkContext)
    }

    /// Liefert den Status aller bekannten Arbeitskontexte.
    ///
    /// Die Liste ist begrenzt und folgt der Commit-Reihenfolge der Aufgaben.
    /// Sie enthält keine Nutzdaten, sondern nur Zustand und Bindungen.
    pub fn work_contexts(&self, host: &Host) -> Result<Vec<WorkContextReport>, HostError> {
        let service = self.context_service(host);
        let mut reports = Vec::new();
        for sequence in self.journal.recent_task_sequences(50)? {
            let Some(context) = service
                .current(&self.config.runtime_id, sequence)
                .map_err(HostError::WorkContext)?
            else {
                continue;
            };
            reports.push(WorkContextReport {
                context_id: context.context_id,
                task_sequence: context.task_sequence,
                route_id: context.route_id,
                state: context.state.as_str().to_owned(),
                goal_id: context.goal_id,
                plan_revision: context.plan_revision,
                priority: context.priority,
                artifact_versions: context.artifact_versions.len(),
                operations: context
                    .operations
                    .iter()
                    .map(|operation| OperationReport {
                        operation_id: operation.operation_id.clone(),
                        kind: operation.kind.clone(),
                        state: operation.state.as_str().to_owned(),
                        version_id: operation.version_id.as_ref().map(|id| id.0.clone()),
                    })
                    .collect(),
                waiting_for: context
                    .waiting_for
                    .iter()
                    .map(|condition| format!("{}:{}", condition.kind.as_str(), condition.reference))
                    .collect(),
                classification: context.classification,
            });
        }
        Ok(reports)
    }
}

/// Stabile Bezeichnung einer Fähigkeit für den Authority-Scope.
fn capability_key(capability: &arcs::adapters::CapabilityRef) -> String {
    format!("{}/{}", capability.adapter_id.0, capability.capability_id.0)
}

//! Task-level error handling shared by the runtime's workers and scheduler.
use super::{FailedTask, HostError, ReasoningRoute, ResourceWait, TaskRuntime, TickReport};
use crate::Host;
use arcs::runtime::work_context::{
    ContextChange, WaitCondition, WaitKind, WorkContextError, WorkContextState,
};

impl TaskRuntime {
    pub(super) fn record_job_failure(
        &mut self,
        host: &Host,
        sequence: u64,
        route: &ReasoningRoute,
        error: HostError,
        report: &mut TickReport,
    ) -> Result<(), HostError> {
        if let HostError::ResourceBusy {
            resource,
            requested,
            available,
        } = error
        {
            self.mark_work_context(
                host,
                sequence,
                WorkContextState::Waiting,
                vec![WaitCondition {
                    kind: WaitKind::Resource,
                    reference: resource.clone(),
                }],
                "exclusive resource is busy",
            )?;
            report.waiting_resources.push(ResourceWait {
                sequence,
                route_id: route.id.clone(),
                resource,
                requested,
                available,
            });
            return Ok(());
        }
        if matches!(error, HostError::AdapterBusy(_)) {
            return Ok(());
        }
        if is_core_failure(&error) {
            return Err(error);
        }
        self.mark_work_context(
            host,
            sequence,
            WorkContextState::Failed,
            Vec::new(),
            "task failed",
        )?;
        self.journal.fail(sequence, &error)?;
        report.failed.push(FailedTask {
            sequence,
            route_id: route.id.clone(),
            error: error.to_string(),
        });
        Ok(())
    }

    /// Schreibt einen Fehler- oder Wartezustand in den Arbeitskontext.
    ///
    /// Das Journal bleibt führ den Aufgabenzustand maßgeblich. Ein
    /// unzulässiger Übergang im Kontext wird deshalb nicht zum Fehler des
    /// Ticks, während Speicherfehler weiterhin sofort auffallen.
    fn mark_work_context(
        &self,
        host: &Host,
        sequence: u64,
        state: WorkContextState,
        waiting_for: Vec<WaitCondition>,
        reason: &str,
    ) -> Result<(), HostError> {
        let Some(current) = self.current_work_context(host, sequence)? else {
            return Ok(());
        };
        match self.advance_work_context(
            host,
            &current,
            &[ContextChange::State {
                state,
                waiting_for,
                reason: reason.to_owned(),
            }],
        ) {
            Ok(_) => Ok(()),
            Err(HostError::WorkContext(WorkContextError::Transition(_))) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

// Persistence/identity failures remain fatal. Isolating a component failure
// must never hide a broken Core store or a changed topology.
pub(super) fn is_core_failure(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Store(_)
            | HostError::Reduction(_)
            | HostError::Input {
                source: arcs::input::InputError::Store(_),
                ..
            }
            | HostError::Registration(arcs::adapters::AdapterRegistryError::Store(_))
            | HostError::Output {
                source: arcs::output::OutputError::Store(_)
                    | arcs::output::OutputError::Invocation(_)
                    | arcs::output::OutputError::StaleDispatch,
                ..
            }
            | HostError::Request {
                source: arcs::request::RequestError::Store(_)
                    | arcs::request::RequestError::Invocation(_)
                    | arcs::request::RequestError::StaleDispatch,
                ..
            }
            | HostError::Execution {
                source: arcs::execution::ExecutionError::Store(_)
                    | arcs::execution::ExecutionError::Invocation(
                        arcs::runtime::InvocationError::Store(_)
                            | arcs::runtime::InvocationError::InvalidState(_)
                            | arcs::runtime::InvocationError::IdentityConflict(_)
                            | arcs::runtime::InvocationError::InvalidTransition { .. }
                            | arcs::runtime::InvocationError::MissingResult
                    )
                    | arcs::execution::ExecutionError::Reservation(
                        arcs::runtime::reservations::ReservationError::Store(_)
                    ),
                ..
            }
            | HostError::Transform {
                source: arcs::transform::TransformError::Store(_)
                    | arcs::transform::TransformError::Invocation(
                        arcs::runtime::InvocationError::Store(_)
                            | arcs::runtime::InvocationError::InvalidState(_)
                            | arcs::runtime::InvocationError::IdentityConflict(_)
                            | arcs::runtime::InvocationError::InvalidTransition { .. }
                            | arcs::runtime::InvocationError::MissingResult
                    ),
                ..
            }
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Store(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Invocation(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Retrieval(
                arcs::store::retrieval::RetrievalError::Store(_)
            ))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::InputData(
                arcs::adapters::input_data::InputDataError::Store(_)
            ))
            | HostError::WorkContext(arcs::runtime::work_context::WorkContextError::Store(_))
    )
}

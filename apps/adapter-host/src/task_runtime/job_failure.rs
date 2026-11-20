//! Task-level error handling shared by the runtime's workers and scheduler.
use super::{FailedTask, HostError, ReasoningRoute, ResourceWait, TaskRuntime, TickReport};

impl TaskRuntime {
    pub(super) fn record_job_failure(
        &mut self,
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
        self.journal.fail(sequence, &error)?;
        report.failed.push(FailedTask {
            sequence,
            route_id: route.id.clone(),
            error: error.to_string(),
        });
        Ok(())
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
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Store(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Invocation(_))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::Retrieval(
                arcs::store::retrieval::RetrievalError::Store(_)
            ))
            | HostError::Reasoning(arcs::reasoning::ReasoningError::InputData(
                arcs::adapters::input_data::InputDataError::Store(_)
            ))
    )
}

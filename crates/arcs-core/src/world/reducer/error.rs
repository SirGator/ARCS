use std::fmt;

use crate::world::belief::StateEstimateError;
use crate::world::entity::EntityError;
use crate::world::observation::ObservationCursor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReduceError {
    Entity(EntityError),
    StateEstimate(StateEstimateError),
    OutOfOrderObservation {
        last: ObservationCursor,
        incoming: ObservationCursor,
    },
    CursorConflict(ObservationCursor),
    RevisionOverflow,
}

impl From<EntityError> for ReduceError {
    fn from(value: EntityError) -> Self {
        Self::Entity(value)
    }
}

impl From<StateEstimateError> for ReduceError {
    fn from(value: StateEstimateError) -> Self {
        Self::StateEstimate(value)
    }
}

impl fmt::Display for ReduceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entity(error) => write!(formatter, "entity reduction failed: {error}"),
            Self::StateEstimate(error) => {
                write!(formatter, "state estimation failed: {error}")
            }
            Self::OutOfOrderObservation { last, incoming } => write!(
                formatter,
                "observation cursor {} is older than applied cursor {}",
                incoming.get(),
                last.get()
            ),
            Self::CursorConflict(cursor) => write!(
                formatter,
                "observation cursor {} identifies conflicting evidence",
                cursor.get()
            ),
            Self::RevisionOverflow => write!(formatter, "world revision overflow"),
        }
    }
}

impl std::error::Error for ReduceError {}

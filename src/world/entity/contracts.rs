use std::fmt;

use crate::core::{SubjectId, VersionId};
use crate::world::observation::RecordedObservation;

/// Maximale Größe einer kanonischen Entity-ID.
///
/// Die erste Policy übernimmt das bereits sicher namespacete Subject einer
/// Observation exakt. Die gemeinsame Grenze verhindert, dass aus einem
/// gültigen Subject eine ungültige interne Identität entsteht.
pub const MAX_ENTITY_ID_BYTES: usize = 512;

/// Stabile Identität eines Objekts in der internen Weltsicht.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(value: impl Into<String>) -> Result<Self, EntityError> {
        let value = value.into();
        if invalid_identity(&value) {
            return Err(EntityError::InvalidId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Ein kanonisch identifiziertes Objekt und seine erste Evidenz.
///
/// Im ersten Vertical Slice ist die Auflösung bewusst exakt und injektiv:
/// ein namespacetes Observation-Subject entspricht genau einer Entity. Eine
/// spätere Alias- oder Sensorfusion wird dadurch zu einer expliziten Policy
/// und nicht zu versteckter Ähnlichkeitsheuristik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    id: EntityId,
    canonical_subject: SubjectId,
    introduced_by: VersionId,
}

impl Entity {
    pub(crate) fn from_observation(observation: &RecordedObservation) -> Result<Self, EntityError> {
        let artifact = observation.artifact();
        let canonical_subject = artifact
            .subject
            .clone()
            .ok_or(EntityError::MissingSubject)?;
        let id = EntityId::new(canonical_subject.0.clone())?;
        Ok(Self {
            id,
            canonical_subject,
            introduced_by: artifact.version_id.clone(),
        })
    }

    pub fn id(&self) -> &EntityId {
        &self.id
    }

    pub fn canonical_subject(&self) -> &SubjectId {
        &self.canonical_subject
    }

    pub fn introduced_by(&self) -> &VersionId {
        &self.introduced_by
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityError {
    MissingSubject,
    InvalidId(String),
}

impl fmt::Display for EntityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSubject => write!(formatter, "recorded observation has no subject"),
            Self::InvalidId(value) => write!(formatter, "invalid entity id '{value}'"),
        }
    }
}

impl std::error::Error for EntityError {}

fn invalid_identity(value: &str) -> bool {
    value.trim().is_empty()
        || value.len() > MAX_ENTITY_ID_BYTES
        || value.chars().any(char::is_control)
}

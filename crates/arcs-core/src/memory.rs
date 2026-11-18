//! Epistemic annotations in the existing Artifact Store. An annotation records
//! an authenticated operator's assessment; it never changes source trust or
//! authorizes an action. Callers must authenticate the supplied human actor.
mod records;
#[cfg(test)]
mod tests;

use crate::core::*;
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "arcs.memory_assessment.v1";
pub const PUBLISHER: &str = "arcs.memory.assessment";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Disputed,
    Retracted,
    Superseded,
}

/// Bounds use canonical UTC seconds; the start is inclusive, the end exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAssessmentCommand {
    pub target_version_id: VersionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version_id: Option<VersionId>,
    pub decision_by: Actor,
    pub reason: String,
    pub status: MemoryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_version_id: Option<VersionId>,
}
impl MemoryAssessmentCommand {
    pub fn validate(&self) -> Result<(), MemoryError> {
        let mut evidence = std::collections::HashSet::new();
        if !valid_id(&self.target_version_id)
            || self
                .expected_version_id
                .as_ref()
                .is_some_and(|id| !valid_id(id))
            || self.decision_by.actor_type != ActorType::Human
            || self.decision_by.id.trim().is_empty()
            || self.decision_by.id.len() > 256
            || self.decision_by.id.chars().any(char::is_control)
            || self.reason.trim().is_empty()
            || self.reason.len() > 2048
            || self.reason.chars().any(char::is_control)
            || self.evidence.len() > 16
            || self
                .evidence
                .iter()
                .any(|id| !valid_id(id) || !evidence.insert(id))
            || self.valid_from.as_ref().is_some_and(|s| !canonical_time(s))
            || self
                .valid_until
                .as_ref()
                .is_some_and(|s| !canonical_time(s))
            || matches!((&self.valid_from, &self.valid_until), (Some(a),Some(b)) if a>=b)
            || (self.status == MemoryStatus::Superseded) != self.replacement_version_id.is_some()
            || self
                .replacement_version_id
                .as_ref()
                .is_some_and(|id| !valid_id(id) || *id == self.target_version_id)
        {
            return Err(invalid("invalid memory assessment command"));
        }
        Ok(())
    }
}
fn valid_id(id: &VersionId) -> bool {
    !id.0.is_empty() && id.0.len() <= 512 && !id.0.chars().any(char::is_control)
}
pub(crate) fn canonical_time(value: &str) -> bool {
    value.len() == 20
        && value.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            10 => b == b'T',
            13 | 16 => b == b':',
            19 => b == b'Z',
            _ => b.is_ascii_digit(),
        })
        && crate::core::schema::is_rfc3339(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAssessment {
    pub version_id: VersionId,
    pub command: MemoryAssessmentCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEpistemicPolicy {
    pub allow_unassessed: bool,
    pub allow_disputed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryDisposition {
    Unassessed,
    Active,
    Disputed,
    Retracted,
    Superseded,
    NotYetValid,
    Expired,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDecision {
    pub target_version_id: VersionId,
    pub assessment_version_id: Option<VersionId>,
    pub disposition: MemoryDisposition,
    pub admitted: bool,
}
#[derive(Debug)]
pub enum MemoryError {
    Store(StoreError),
    Invalid(String),
    Conflict,
}
impl From<StoreError> for MemoryError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
fn invalid(message: &str) -> MemoryError {
    MemoryError::Invalid(message.into())
}

pub struct MemoryAssessmentService<'a> {
    pub store: &'a SqliteArtifactStore,
    pub schemas: &'a SchemaRegistry,
    pub clock: &'a dyn Clock,
}
impl MemoryAssessmentService<'_> {
    /// Exact replay of the latest command returns its original receipt. A
    /// stale command after another revision conflicts rather than reapplying.
    pub fn assess(
        &self,
        command: &MemoryAssessmentCommand,
    ) -> Result<MemoryAssessment, MemoryError> {
        command.validate()?;
        records::validate_references(self, command)?;
        let previous = records::load_at(
            self,
            &command.target_version_id,
            self.store.latest_sequence()?,
        )?;
        if let Some((_, receipt)) = &previous
            && receipt.command == *command
        {
            return Ok(receipt.clone());
        }
        if previous.as_ref().map(|(_, r)| &r.version_id) != command.expected_version_id.as_ref() {
            return Err(MemoryError::Conflict);
        }
        records::commit(self, command, previous.as_ref().map(|(a, _)| a))
    }
    pub fn current(&self, target: &VersionId) -> Result<Option<MemoryAssessment>, MemoryError> {
        if !valid_id(target) {
            return Err(invalid("invalid memory target identity"));
        }
        Ok(records::load_at(self, target, self.store.latest_sequence()?)?.map(|(_, r)| r))
    }
    /// Evaluate immutable evidence as it existed at the retrieval cursor. A
    /// later assessment cannot silently alter a saved context or its replay.
    pub fn evaluate(
        &self,
        target: &VersionId,
        through: u64,
        at: &str,
        policy: &MemoryEpistemicPolicy,
    ) -> Result<MemoryDecision, MemoryError> {
        if !valid_id(target) || !canonical_time(at) {
            return Err(invalid("invalid memory evaluation identity or time"));
        }
        let assessment = records::load_at(self, target, through)?.map(|(_, r)| r);
        let disposition = match assessment.as_ref().map(|r| &r.command) {
            None => MemoryDisposition::Unassessed,
            Some(c) if c.status == MemoryStatus::Retracted => MemoryDisposition::Retracted,
            Some(c) if c.status == MemoryStatus::Superseded => MemoryDisposition::Superseded,
            Some(c) if c.valid_from.as_deref().is_some_and(|s| at < s) => {
                MemoryDisposition::NotYetValid
            }
            Some(c) if c.valid_until.as_deref().is_some_and(|s| at >= s) => {
                MemoryDisposition::Expired
            }
            Some(c) if c.status == MemoryStatus::Disputed => MemoryDisposition::Disputed,
            Some(_) => MemoryDisposition::Active,
        };
        let admitted = match disposition {
            MemoryDisposition::Unassessed => policy.allow_unassessed,
            MemoryDisposition::Disputed => policy.allow_disputed,
            MemoryDisposition::Active => true,
            _ => false,
        };
        Ok(MemoryDecision {
            target_version_id: target.clone(),
            assessment_version_id: assessment.map(|r| r.version_id),
            disposition,
            admitted,
        })
    }
}

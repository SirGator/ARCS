//! Operator entry point for annotations in the shared Core. Local CLI access
//! authenticates through the host account; embedded callers authenticate actors.
use crate::{Host, HostError};
use arcs::core::{SystemClock, VersionId};
use arcs::memory::{
    MemoryAssessment, MemoryAssessmentCommand, MemoryAssessmentService, MemoryError,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryCommand {
    Assess { assessment: MemoryAssessmentCommand },
    Inspect { target_version_ids: Vec<VersionId> },
}
impl MemoryCommand {
    pub fn validate(&self) -> Result<(), HostError> {
        match self {
            Self::Assess { assessment } => assessment.validate().map_err(HostError::Memory),
            Self::Inspect { target_version_ids } => validate_targets(target_version_ids),
        }
    }
}
fn validate_targets(targets: &[VersionId]) -> Result<(), HostError> {
    let mut seen = std::collections::HashSet::new();
    if !(1..=32).contains(&targets.len())
        || targets.iter().any(|id| {
            id.0.is_empty()
                || id.0.len() > 512
                || id.0.chars().any(char::is_control)
                || !seen.insert(id)
        })
    {
        return Err(HostError::Memory(MemoryError::Invalid(
            "inspect needs 1..32 distinct version IDs".into(),
        )));
    }
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct MemoryInspection {
    pub target_version_id: VersionId,
    pub assessment: Option<MemoryAssessment>,
}
impl Host {
    pub fn assess_memory(
        &self,
        command: &MemoryAssessmentCommand,
    ) -> Result<MemoryAssessment, HostError> {
        MemoryAssessmentService {
            store: self.store(),
            schemas: self.schemas(),
            clock: &SystemClock,
        }
        .assess(command)
        .map_err(HostError::Memory)
    }
    pub fn inspect_memory(
        &self,
        target_version_ids: &[VersionId],
    ) -> Result<Vec<MemoryInspection>, HostError> {
        validate_targets(target_version_ids)?;
        let service = MemoryAssessmentService {
            store: self.store(),
            schemas: self.schemas(),
            clock: &SystemClock,
        };
        target_version_ids
            .iter()
            .map(|target| {
                Ok(MemoryInspection {
                    target_version_id: target.clone(),
                    assessment: service.current(target).map_err(HostError::Memory)?,
                })
            })
            .collect()
    }
}

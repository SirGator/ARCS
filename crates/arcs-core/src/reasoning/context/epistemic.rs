use super::*;
use crate::memory::{MemoryAssessmentService, MemoryDecision};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EpistemicTrace {
    pub evaluated_at: String,
    pub decisions: Vec<MemoryDecision>,
}

impl ReasoningContextService<'_> {
    pub(super) fn admit_memory(
        &self,
        record: &mut ContextRecord,
        target: &VersionId,
    ) -> Result<bool, ReasoningError> {
        let Some(policy) = &record.policy.epistemic else {
            return Ok(true);
        };
        let trace = record
            .epistemic
            .as_mut()
            .ok_or_else(|| invalid("missing epistemic memory trace"))?;
        let decision = MemoryAssessmentService {
            store: self.store,
            schemas: self.schemas,
            clock: self.clock,
        }
        .evaluate(
            target,
            record.trace.artifact_cursor,
            &trace.evaluated_at,
            policy,
        )
        .map_err(memory_error)?;
        let admitted = decision.admitted;
        trace.decisions.push(decision);
        Ok(admitted)
    }
    pub(super) fn validate_epistemic_record(
        &self,
        record: &ContextRecord,
    ) -> Result<(), ReasoningError> {
        match (&record.policy.epistemic, &record.epistemic) {
            (None, None) => Ok(()),
            (Some(policy), Some(trace)) => {
                if trace.decisions.len() > record.policy.search.max_candidates {
                    return Err(invalid("epistemic memory trace exceeds candidate limit"));
                }
                let mut seen = HashSet::new();
                let service = MemoryAssessmentService {
                    store: self.store,
                    schemas: self.schemas,
                    clock: self.clock,
                };
                for decision in &trace.decisions {
                    if !seen.insert(&decision.target_version_id)
                        || service
                            .evaluate(
                                &decision.target_version_id,
                                record.trace.artifact_cursor,
                                &trace.evaluated_at,
                                policy,
                            )
                            .map_err(memory_error)?
                            != *decision
                    {
                        return Err(invalid("saved epistemic memory evidence changed"));
                    }
                }
                for selected in &record.selected {
                    if !trace
                        .decisions
                        .iter()
                        .any(|d| d.target_version_id == selected.selection.version_id && d.admitted)
                    {
                        return Err(invalid("saved memory was not admitted by epistemic policy"));
                    }
                }
                // Also validate the time when the search returned no candidates.
                if !crate::memory::canonical_time(&trace.evaluated_at) {
                    return Err(invalid("invalid saved epistemic evaluation time"));
                }
                Ok(())
            }
            _ => Err(invalid("epistemic policy/trace mismatch")),
        }
    }
}
fn memory_error(error: crate::memory::MemoryError) -> ReasoningError {
    match error {
        crate::memory::MemoryError::Store(e) => ReasoningError::Store(e),
        other => invalid(&format!("memory assessment: {other:?}")),
    }
}

//! Bounded operator queries over verified attempt evidence.
use super::*;
use crate::store::SqliteArtifactStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptQuery {
    pub capabilities: Vec<CapabilityRef>,
    #[serde(default)]
    pub after_sequence: u64,
    pub limit: usize,
}
impl AttemptQuery {
    pub fn validate(&self) -> Result<(), ReasoningError> {
        let mut seen = std::collections::HashSet::new();
        if !(1..=16).contains(&self.capabilities.len())
            || !(1..=256).contains(&self.limit)
            || self.after_sequence > i64::MAX as u64
            || self.capabilities.iter().any(|cap| {
                [&cap.adapter_id.0, &cap.capability_id.0].iter().any(|id| {
                    id.is_empty()
                        || id.len() > 128
                        || !id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
                }) || !seen.insert(cap)
            })
        {
            return Err(invalid(
                "attempt query needs 1..16 distinct capabilities and a page size of 1..256",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct AttemptEntry {
    pub sequence: u64,
    pub version_id: VersionId,
    pub recorded_at: String,
    pub attempt: ReasoningAttempt,
}
#[derive(Debug, Default, Serialize)]
pub struct AttemptPageCounts {
    pub valid_responses: usize,
    pub solved_candidate_responses: usize,
    pub failures: usize,
    pub retryable_failures: usize,
}
#[derive(Debug, Serialize)]
pub struct AttemptPage {
    pub coverage: &'static str,
    pub through_sequence: u64,
    pub attempts: Vec<AttemptEntry>,
    pub page_counts: AttemptPageCounts,
    pub next_sequence: u64,
    pub scanned: usize,
    pub skipped_untrusted: usize,
    pub skipped_oversized: usize,
    pub has_more: bool,
    /// Response/schema validity is not a measurement of goal success.
    pub candidate_quality_verified: bool,
}

pub fn history(
    registry: &crate::adapters::AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    query: &AttemptQuery,
) -> Result<AttemptPage, ReasoningError> {
    query.validate()?;
    for cap in &query.capabilities {
        let (_, descriptor) =
            registry.authorized_capability(&cap.adapter_id, &cap.capability_id)?;
        if !descriptor.contract.is_reasoning() {
            return Err(invalid("attempt history requires a Reason capability"));
        }
    }
    let through = store.latest_sequence()?;
    let mut candidates = std::collections::BTreeMap::new();
    for cap in &query.capabilities {
        for row in store.schema_subject_page_after(
            &SchemaId(SCHEMA.into()),
            &capability_subject(cap),
            query.after_sequence,
            through,
            query.limit + 1,
            32_768,
        )? {
            candidates.entry(row.sequence).or_insert(row);
        }
    }
    let mut page = AttemptPage {
        coverage: "recorded_completed_v2_attempts",
        through_sequence: through,
        attempts: vec![],
        page_counts: Default::default(),
        next_sequence: query.after_sequence,
        scanned: 0,
        skipped_untrusted: 0,
        skipped_oversized: 0,
        has_more: candidates.len() > query.limit,
        candidate_quality_verified: false,
    };
    for (sequence, row) in candidates.into_iter().take(query.limit) {
        page.scanned += 1;
        page.next_sequence = sequence;
        let Some(artifact) = row.artifact else {
            page.skipped_oversized += 1;
            continue;
        };
        if !trusted(&artifact, PUBLISHER) {
            page.skipped_untrusted += 1;
            continue;
        }
        let record = read(store, schemas, &artifact)?;
        if !query.capabilities.contains(&record.capability) {
            page.skipped_untrusted += 1;
            continue;
        }
        match &record.outcome {
            AttemptOutcome::Response { status, .. } => {
                page.page_counts.valid_responses += 1;
                page.page_counts.solved_candidate_responses +=
                    usize::from(*status == ReasoningStatus::Solved);
            }
            AttemptOutcome::Failed { retryable, .. } => {
                page.page_counts.failures += 1;
                page.page_counts.retryable_failures += usize::from(*retryable);
            }
        }
        page.attempts.push(AttemptEntry {
            sequence,
            version_id: artifact.version_id,
            recorded_at: artifact.created_at,
            attempt: record,
        });
    }
    if !page.has_more {
        page.next_sequence = through.max(query.after_sequence);
    }
    Ok(page)
}

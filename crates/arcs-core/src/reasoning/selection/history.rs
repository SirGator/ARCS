//! Technical attempt evidence can break a tie among equally relevant ready choices.
use super::*;
use crate::reasoning::attempts::{self, AttemptOutcome, ReasoningFailureKind};

const MAX_LOOKBACK: usize = 16;
const SCALE: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRanking {
    pub lookback: usize,
    pub min_samples: usize,
    pub failure_weight: u8,
    pub latency_weight: u8,
    pub latency_reference_ms: u64,
}

impl HistoryRanking {
    pub(super) fn validate(&self) -> Result<(), ReasoningError> {
        if !(1..=MAX_LOOKBACK).contains(&self.lookback)
            || !(1..=self.lookback).contains(&self.min_samples)
            || !(1..=100).contains(&self.failure_weight)
            || self.latency_weight > 100
            || !(1..=600_000).contains(&self.latency_reference_ms)
        {
            return Err(invalid("invalid technical history ranking bounds"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEvidence {
    pub sample_versions: Vec<VersionId>,
    pub failures: usize,
    pub latency_samples: usize,
    pub mean_latency_ms: Option<u64>,
}

pub(super) fn observe(
    store: &SqliteArtifactStore,
    schemas: &SchemaRegistry,
    choice: &ReasonerChoice,
    target: &SchemaId,
    through: u64,
    limit: usize,
) -> Result<HistoryEvidence, ReasoningError> {
    let mut sample = HistoryEvidence {
        sample_versions: vec![],
        failures: 0,
        latency_samples: 0,
        mean_latency_ms: None,
    };
    let mut total_latency = 0u128;
    let rows = store.schema_subject_recent(
        &SchemaId(attempts::SCHEMA.into()),
        &attempts::capability_subject(&choice.capability),
        through,
        limit,
        32_768,
    )?;
    for artifact in rows.into_iter().filter_map(|row| row.artifact) {
        if !attempts::trusted(&artifact, attempts::PUBLISHER) {
            continue;
        }
        let (attempt, recorded_target) = attempts::read_with_target(store, schemas, &artifact)?;
        if attempt.capability != choice.capability
            || attempt.method != choice.method
            || attempt.budget != choice.budget
            || recorded_target.as_ref() != Some(target)
        {
            continue;
        }
        match attempt.outcome {
            AttemptOutcome::Response { .. } => {
                if let Some(latency) = attempt.host_completion_latency_ms {
                    sample.latency_samples += 1;
                    total_latency += u128::from(latency);
                }
            }
            AttemptOutcome::Failed {
                failure:
                    ReasoningFailureKind::AdapterTimeout
                    | ReasoningFailureKind::AdapterUnavailable
                    | ReasoningFailureKind::AdapterRejected
                    | ReasoningFailureKind::InvalidResponse,
                ..
            } => sample.failures += 1,
            AttemptOutcome::Failed { .. } => continue,
        }
        sample.sample_versions.push(artifact.version_id);
    }
    sample.mean_latency_ms = total_latency
        .checked_div(sample.latency_samples as u128)
        .map(|mean| mean as u64);
    Ok(sample)
}

pub(super) fn top_tier(observations: &[ReasonerObservation]) -> Vec<usize> {
    let ready = observations
        .iter()
        .enumerate()
        .filter(|(_, option)| option.matched && option.availability == ReasonerAvailability::Ready);
    let best = ready
        .clone()
        .map(|(_, option)| option.relevance_score)
        .max();
    ready
        .filter_map(|(index, option)| (Some(option.relevance_score) == best).then_some(index))
        .collect()
}

pub(super) fn pick(
    tier: &[usize],
    observations: &[ReasonerObservation],
    ranking: Option<&HistoryRanking>,
) -> usize {
    let first = tier[0];
    let Some(ranking) = ranking else {
        return first;
    };
    let comparable = tier
        .iter()
        .map(|&index| observations[index].history.as_ref());
    if comparable.clone().any(|sample| {
        sample.is_none_or(|sample| sample.sample_versions.len() < ranking.min_samples)
    }) {
        return first;
    }
    let include_latency = ranking.latency_weight > 0
        && comparable.clone().flatten().all(|sample| {
            sample.latency_samples >= ranking.min_samples && sample.mean_latency_ms.is_some()
        });
    tier.iter()
        .copied()
        .zip(comparable.flatten())
        .min_by_key(|(index, sample)| {
            let failures = sample.failures as u64 * SCALE / sample.sample_versions.len() as u64;
            let latency = if include_latency {
                sample
                    .mean_latency_ms
                    .unwrap_or(0)
                    .min(ranking.latency_reference_ms)
                    * SCALE
                    / ranking.latency_reference_ms
            } else {
                0
            };
            (
                failures * u64::from(ranking.failure_weight)
                    + latency * u64::from(ranking.latency_weight),
                *index,
            )
        })
        .map_or(first, |(index, _)| index)
}

pub(super) fn recorded_samples_valid(record: &SelectionRecord, tier: &[usize]) -> bool {
    let enabled = record.policy.history.is_some() && tier.len() > 1;
    if enabled != record.history_through_sequence.is_some() {
        return false;
    }
    record
        .observations
        .iter()
        .enumerate()
        .all(|(index, option)| {
            let collect = enabled && tier.contains(&index);
            match (&option.history, &record.policy.history) {
                (Some(sample), Some(config)) if collect => {
                    let count = sample.sample_versions.len();
                    count <= config.lookback
                        && sample.failures <= count
                        && sample.latency_samples <= count - sample.failures
                        && sample.mean_latency_ms.is_some() == (sample.latency_samples > 0)
                        && sample.sample_versions.iter().collect::<HashSet<_>>().len() == count
                }
                (None, _) => !collect,
                _ => false,
            }
        })
}

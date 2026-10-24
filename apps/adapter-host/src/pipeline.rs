//! Explizite lineare Pipelines. Keine eingebauten Freigabepolicies oder Geräte.

mod journal;

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use arcs::adapters::{AdapterManifest, CapabilityContract, CapabilityRef, ProducerClass};
use arcs::core::{Artifact, SchemaId, Trust, VersionId};
use arcs::world::observation::ObservationLog;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Host, HostError, HostReport};
use journal::{ActiveInput, Journal};

#[derive(Debug)]
pub enum PipelineError {
    Host(HostError),
    Invalid(String),
    Policy(String),
}
impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(f, "{error}"),
            Self::Invalid(message) => write!(f, "pipeline: {message}"),
            Self::Policy(message) => write!(f, "pipeline policy: {message}"),
        }
    }
}
impl std::error::Error for PipelineError {}
impl From<HostError> for PipelineError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}
type Result<T> = std::result::Result<T, PipelineError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformStep {
    pub id: String,
    pub capability: CapabilityRef,
    pub output_schema_id: SchemaId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    pub config_version: u32,
    pub pipeline_id: String,
    pub source: CapabilityRef,
    pub input_schema_id: SchemaId,
    pub steps: Vec<TransformStep>,
    pub output: CapabilityRef,
    pub receipt_schema_id: SchemaId,
    /// Budget neuer oder unterbrochener Eingaben pro Lauf, nicht der Historie.
    pub max_observations: usize,
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub id: String,
    pub input_version_id: VersionId,
    pub version_id: VersionId,
    pub schema_id: SchemaId,
    pub trust: Trust,
}

#[derive(Debug, Serialize)]
pub struct Delivery {
    pub input_version_id: VersionId,
    pub steps: Vec<StepReport>,
    pub transformed_version_id: VersionId,
    pub transformed_trust: Trust,
    pub request_version_id: VersionId,
    pub approval_version_id: VersionId,
    pub receipt_version_id: VersionId,
    pub receipt: Value,
}

#[derive(Debug, Serialize)]
pub struct PipelineReport {
    pub pipeline_id: String,
    pub definition_version_id: VersionId,
    pub checkpoint_version_id: Option<VersionId>,
    pub completed_cursor: u64,
    pub remaining_observations: usize,
    /// Nur in diesem Lauf abgeschlossene Zustellungen.
    pub deliveries: Vec<Delivery>,
    pub host: HostReport,
}

/// Der Anwendung übergebener, bereits persistierter Ausgabeauftrag.
pub struct PipelineOutput<'a> {
    pub config: &'a PipelineConfig,
    pub input: &'a Artifact,
    pub transformed: &'a Artifact,
    pub steps: &'a [StepReport],
    pub request: &'a Artifact,
    /// Bei Wiederaufnahme muss dieselbe Freigabe erneut bewertet werden.
    pub approval_version_id: Option<&'a VersionId>,
}

/// Nur die Anwendung darf die Exportentscheidung treffen. Eine Rückgabe ist
/// noch keine Zustellung; der OutputService prüft die Freigabe vollständig.
/// Die Rückgabe muss exakt einem bereits gespeicherten Artefakt entsprechen.
pub trait PipelineApproval {
    fn approve(&mut self, host: &Host, output: &PipelineOutput<'_>) -> Result<Artifact>;
}

fn invalid(message: impl Into<String>) -> PipelineError {
    PipelineError::Invalid(message.into())
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

impl PipelineConfig {
    fn identity(&self, host: &Host) -> Result<Value> {
        if self.config_version != 2
            || !identifier(&self.pipeline_id)
            || !(1..=1000).contains(&self.max_observations)
            || !(1..=32).contains(&self.steps.len())
        {
            return Err(invalid(
                "requires config_version 2, a valid pipeline_id, 1..32 steps and max_observations 1..1000",
            ));
        }
        let mut step_ids = HashSet::new();
        let mut manifests: BTreeMap<String, AdapterManifest> = BTreeMap::new();
        let mut contract =
            |capability: &CapabilityRef| -> Result<(ProducerClass, CapabilityContract)> {
                let (registered, descriptor) = host
                    .registry()
                    .authorized_capability(&capability.adapter_id, &capability.capability_id)
                    .map_err(HostError::Registration)?;
                manifests.insert(
                    capability.adapter_id.0.clone(),
                    registered.manifest().clone(),
                );
                Ok((
                    registered.grant().producer_class,
                    descriptor.contract.clone(),
                ))
            };
        let (_, source) = contract(&self.source)?;
        if !matches!(source, CapabilityContract::Observe { emits }
            if emits.as_slice() == std::slice::from_ref(&self.input_schema_id))
        {
            return Err(invalid(
                "source must Observe exactly the configured input schema",
            ));
        }
        let mut input_schema = &self.input_schema_id;
        for step in &self.steps {
            if !identifier(&step.id) || !step_ids.insert(&step.id) {
                return Err(invalid("step IDs must be valid and unique"));
            }
            let (producer, transform) = contract(&step.capability)?;
            if producer == ProducerClass::Model {
                return Err(invalid("models must use the reasoning port"));
            }
            if !matches!(transform, CapabilityContract::Transform { accepts, emits }
                if accepts.contains(input_schema) && emits.contains(&step.output_schema_id))
            {
                return Err(invalid(format!(
                    "schema contract mismatch at step {}",
                    step.id
                )));
            }
            input_schema = &step.output_schema_id;
        }
        let (producer, output) = contract(&self.output)?;
        if producer != ProducerClass::Executor
            || !matches!(output,
            CapabilityContract::Output { accepts, emits, idempotent: true }
            if accepts.contains(input_schema) && emits.contains(&self.receipt_schema_id))
        {
            return Err(invalid(
                "output requires an Executor and matching idempotent Output contract",
            ));
        }
        // Grants und Laufbudget sind veränderbar; Topologie und Manifeste
        // bilden die dauerhafte Identität. Schema-Drift prüft bereits der Store.
        Ok(
            json!({"source":self.source,"input_schema_id":self.input_schema_id,"steps":self.steps,
            "output":self.output,"receipt_schema_id":self.receipt_schema_id,"manifests":manifests}),
        )
    }
}

/// Prüft die gesamte Topologie vor dem Polling. Fortschritt liegt im Core-
/// Store als interne Input-Ereignisse; erfolgreiche Core-Aufrufe sind dedupliziert.
/// Ein Fehler stoppt den Lauf an der aktuellen Eingabe. Es gibt keine Retries
/// innerhalb dieses Aufrufs und keine automatische Freigabe.
pub fn run_pipeline(
    host: &mut Host,
    config: &PipelineConfig,
    policy: &mut dyn PipelineApproval,
) -> Result<PipelineReport> {
    let identity = config.identity(host)?;
    let mut journal = Journal::open(host, config, identity)?;
    if let Err(error) = host.poll_round() {
        journal.state.error = Some(format!("polling: {error}").chars().take(2000).collect());
        journal.save(host)?;
        return Err(error.into());
    }
    if journal.state.active.is_none() && journal.state.error.is_some() {
        journal.state.error = None;
        journal.save(host)?;
    }
    let capability_tag = format!("capability:{}", config.source.capability_id.0);
    let pending: Vec<_> = ObservationLog::new(host.store())
        .after(None)
        .map_err(HostError::Store)?
        .into_iter()
        .filter(|recorded| recorded.cursor().get() > journal.state.cursor)
        .filter(|recorded| {
            let artifact = recorded.artifact();
            artifact.created_by.id == config.source.adapter_id.0
                && artifact.schema_id == config.input_schema_id
                && artifact.tags.contains(&capability_tag)
        })
        .collect();
    if let Some(active) = &journal.state.active
        && !pending.first().is_some_and(|recorded| {
            recorded.cursor().get() == active.cursor
                && recorded.artifact().version_id == active.input_version_id
        })
    {
        return Err(invalid("active input no longer matches the source log"));
    }
    let mut deliveries = Vec::new();
    for recorded in pending.iter().take(config.max_observations) {
        if journal.state.active.is_none() {
            journal.state.active = Some(ActiveInput::new(
                recorded.artifact().version_id.clone(),
                recorded.cursor().get(),
            ));
            journal.save(host)?;
        }
        match advance(host, config, recorded.artifact(), &mut journal, policy) {
            Ok(delivery) => deliveries.push(delivery),
            Err(error) => {
                journal.state.error = Some(
                    format!("{}: {error}", journal.phase)
                        .chars()
                        .take(2000)
                        .collect(),
                );
                journal.save(host)?;
                return Err(error);
            }
        }
    }
    Ok(PipelineReport {
        pipeline_id: config.pipeline_id.clone(),
        definition_version_id: journal.definition.version_id,
        checkpoint_version_id: journal.checkpoint_version_id,
        completed_cursor: journal.state.cursor,
        remaining_observations: pending.len() - deliveries.len(),
        deliveries,
        host: host.report()?,
    })
}

fn advance(
    host: &mut Host,
    config: &PipelineConfig,
    input: &Artifact,
    journal: &mut Journal,
    policy: &mut dyn PipelineApproval,
) -> Result<Delivery> {
    let mut transformed = input.clone();
    let mut steps = Vec::new();
    for (index, step) in config.steps.iter().enumerate() {
        journal.phase = format!("transform:{}", step.id);
        // Bereits abgeschlossene Schritte gehen durch den Core-Cache: aktuelle
        // Grants, Trust-Limits und Identität werden vor Wiederverwendung geprüft.
        let result = host.transform(
            &step.capability,
            &transformed.version_id,
            &step.output_schema_id,
        )?;
        let active = journal.state.active.as_mut().unwrap();
        match active.step_versions.get(index) {
            Some(saved) if *saved != result.version_id => {
                return Err(invalid("saved transform result changed identity"));
            }
            Some(_) => {}
            None => {
                active.step_versions.push(result.version_id.clone());
                journal.save(host)?;
            }
        }
        steps.push(StepReport {
            id: step.id.clone(),
            input_version_id: transformed.version_id,
            version_id: result.version_id.clone(),
            schema_id: result.schema_id.clone(),
            trust: result.trust.clone(),
        });
        transformed = result;
    }
    journal.phase = "prepare_output".into();
    let request = host.prepare_output(
        &config.output,
        &transformed.version_id,
        &config.receipt_schema_id,
    )?;
    let active = journal.state.active.as_mut().unwrap();
    match &active.request_version_id {
        Some(saved) if *saved != request.version_id => {
            return Err(invalid("saved output request changed identity"));
        }
        Some(_) => {}
        None => {
            active.request_version_id = Some(request.version_id.clone());
            journal.save(host)?;
        }
    }
    journal.phase = "approval".into();
    let approval = policy.approve(
        host,
        &PipelineOutput {
            config,
            input,
            transformed: &transformed,
            steps: &steps,
            request: &request,
            approval_version_id: journal
                .state
                .active
                .as_ref()
                .unwrap()
                .approval_version_id
                .as_ref(),
        },
    )?;
    if host
        .store()
        .get(&approval.version_id)
        .map_err(HostError::Store)?
        .as_ref()
        != Some(&approval)
    {
        return Err(PipelineError::Policy(
            "approval callback must return an exact persisted artifact".into(),
        ));
    }
    let active = journal.state.active.as_mut().unwrap();
    match &active.approval_version_id {
        Some(saved) if *saved != approval.version_id => {
            return Err(invalid("resumed policy changed the bound approval"));
        }
        Some(_) => {}
        None => {
            active.approval_version_id = Some(approval.version_id.clone());
            journal.save(host)?;
        }
    }
    journal.phase = "deliver_output".into();
    let receipt = host.deliver_output(
        &config.output.adapter_id,
        &request.version_id,
        &approval.version_id,
    )?;
    let active = journal.state.active.as_mut().unwrap();
    match &active.receipt_version_id {
        Some(saved) if *saved != receipt.version_id => {
            return Err(invalid("saved receipt changed identity"));
        }
        Some(_) => {}
        None => {
            active.receipt_version_id = Some(receipt.version_id.clone());
            journal.save(host)?;
        }
    }
    journal.phase = "complete".into();
    journal.state.cursor = journal.state.active.take().unwrap().cursor;
    journal.state.error = None;
    journal.save(host)?;
    Ok(Delivery {
        input_version_id: input.version_id.clone(),
        steps,
        transformed_version_id: transformed.version_id,
        transformed_trust: transformed.trust,
        request_version_id: request.version_id,
        approval_version_id: approval.version_id,
        receipt_version_id: receipt.version_id,
        receipt: receipt.payload,
    })
}

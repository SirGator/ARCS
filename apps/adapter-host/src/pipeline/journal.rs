use arcs::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, ProducerClass,
};
use arcs::core::{
    ActorType, Artifact, SchemaId, SequenceIdGenerator, SourceClass, SourceKind, SystemClock,
    TrustLevel, VersionId, validate_artifact,
};
use arcs::input::{InputMessage, InputService};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{PipelineConfig, Result, invalid};
use crate::config::PIPELINE_ID;
use crate::{Host, HostError};

const SCHEMA: &str = "arcs.input.host_pipeline.v1";
const CAPABILITY: &str = "pipeline.state";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActiveInput {
    pub input_version_id: VersionId,
    pub cursor: u64,
    pub step_versions: Vec<VersionId>,
    pub request_version_id: Option<VersionId>,
    pub approval_version_id: Option<VersionId>,
    pub receipt_version_id: Option<VersionId>,
}
impl ActiveInput {
    pub fn new(input_version_id: VersionId, cursor: u64) -> Self {
        Self {
            input_version_id,
            cursor,
            step_versions: vec![],
            request_version_id: None,
            approval_version_id: None,
            receipt_version_id: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Progress {
    pub cursor: u64,
    pub active: Option<ActiveInput>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    revision: u64,
    definition_version_id: VersionId,
    progress: Progress,
}

pub(super) struct Journal {
    registry: AdapterRegistry,
    ids: SequenceIdGenerator,
    pipeline_id: String,
    revision: u64,
    saved: Progress,
    pub definition: Artifact,
    pub checkpoint_version_id: Option<VersionId>,
    pub state: Progress,
    pub phase: String,
}

impl Journal {
    pub fn open(host: &Host, config: &PipelineConfig, identity: Value) -> Result<Self> {
        let mut registry = AdapterRegistry::new();
        registry
            .register(
                AdapterManifest {
                    protocol_version: 1,
                    adapter_id: AdapterId(PIPELINE_ID.into()),
                    adapter_version: "1".into(),
                    capabilities: vec![CapabilityDescriptor {
                        id: CapabilityId(CAPABILITY.into()),
                        contract: CapabilityContract::Input {
                            emits: vec![SchemaId(SCHEMA.into())],
                        },
                        required_permissions: vec![],
                    }],
                },
                AdapterGrant {
                    adapter_id: AdapterId(PIPELINE_ID.into()),
                    producer_class: ProducerClass::System,
                    enabled_capabilities: vec![CapabilityId(CAPABILITY.into())],
                    granted_permissions: vec![],
                    assigned_trust: TrustLevel::High,
                    ingress_source_kind: Some(SourceKind::Internal),
                    max_payload_bytes: 65_536,
                    max_external_reference_bytes: 256,
                    reasoning_limits_v2: None,
                    reasoning_limits: None,
                    data_limits: None,
                },
                host.schemas(),
                host.store(),
            )
            .map_err(HostError::Registration)?;
        let mut ids = host
            .store()
            .durable_id_generator("pipeline-journal")
            .map_err(HostError::Store)?;
        let definition = InputService::new(&registry,host.schemas(),host.store(),&mut ids,&SystemClock)
            .ingest(&AdapterId(PIPELINE_ID.into()), InputMessage {
                capability_id:CapabilityId(CAPABILITY.into()), external_subject:Some(config.pipeline_id.clone()),
                external_reference:format!("pipeline:{}:definition", config.pipeline_id),
                payload:json!({"kind":"definition","pipeline_id":config.pipeline_id,"data":identity}),
            }).map_err(|source| HostError::Input {request:config.pipeline_id.clone(),source})?;
        validate_record(host, &definition)?;
        let history = host
            .store()
            .history(
                definition.subject.as_ref().unwrap(),
                &SchemaId(SCHEMA.into()),
            )
            .map_err(HostError::Store)?;
        let mut state = Progress::default();
        let mut revision = 0;
        let mut checkpoint_version_id = None;
        for artifact in history {
            validate_record(host, &artifact)?;
            if artifact.payload["kind"] == "definition" {
                continue;
            }
            if artifact.payload["pipeline_id"] != config.pipeline_id {
                return Err(invalid("checkpoint belongs to another pipeline"));
            }
            let checkpoint: Checkpoint =
                serde_json::from_value(artifact.payload["data"].clone())
                    .map_err(|error| invalid(format!("invalid checkpoint: {error}")))?;
            if checkpoint.revision != revision + 1
                || checkpoint.definition_version_id != definition.version_id
                || checkpoint.progress.cursor < state.cursor
            {
                return Err(invalid(
                    "checkpoint identity, revision or cursor is inconsistent",
                ));
            }
            state = checkpoint.progress;
            revision = checkpoint.revision;
            checkpoint_version_id = Some(artifact.version_id);
        }
        if let Some(active) = &state.active
            && (active.cursor <= state.cursor
                || active.step_versions.len() > config.steps.len()
                || (active.request_version_id.is_some()
                    && active.step_versions.len() != config.steps.len())
                || (active.approval_version_id.is_some() && active.request_version_id.is_none())
                || (active.receipt_version_id.is_some() && active.approval_version_id.is_none()))
        {
            return Err(invalid("checkpoint has inconsistent active stages"));
        }
        Ok(Self {
            registry,
            ids,
            pipeline_id: config.pipeline_id.clone(),
            revision,
            saved: state.clone(),
            definition,
            checkpoint_version_id,
            state,
            phase: "pipeline".into(),
        })
    }

    pub fn save(&mut self, host: &Host) -> Result<()> {
        if self.saved == self.state {
            return Ok(());
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("checkpoint revision overflow"))?;
        let artifact = InputService::new(&self.registry,host.schemas(),host.store(),&mut self.ids,&SystemClock)
            .ingest(&AdapterId(PIPELINE_ID.into()), InputMessage {
                capability_id:CapabilityId(CAPABILITY.into()),external_subject:Some(self.pipeline_id.clone()),
                external_reference:format!("pipeline:{}:checkpoint:{revision}",self.pipeline_id),
                payload:json!({"kind":"checkpoint","pipeline_id":self.pipeline_id,"data":{
                    "revision":revision,"definition_version_id":self.definition.version_id,"progress":self.state}}),
            }).map_err(|source| HostError::Input {request:self.pipeline_id.clone(),source})?;
        validate_record(host, &artifact)?;
        self.revision = revision;
        self.saved = self.state.clone();
        self.checkpoint_version_id = Some(artifact.version_id);
        Ok(())
    }
}

fn validate_record(host: &Host, artifact: &Artifact) -> Result<()> {
    validate_artifact(artifact, host.schemas())
        .map_err(|error| invalid(format!("invalid pipeline record: {error:?}")))?;
    if artifact.subject.is_none()
        || artifact.schema_id.0 != SCHEMA
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PIPELINE_ID
        || artifact.source.kind != SourceKind::Internal
        || artifact.trust.level != TrustLevel::High
        || artifact.trust.source_class != SourceClass::System
    {
        return Err(invalid("pipeline record must be an internal host event"));
    }
    Ok(())
}

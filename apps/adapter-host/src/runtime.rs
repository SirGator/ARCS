use std::path::Path;
use std::process::Command;
use std::time::Duration;

use arcs::adapters::stdio::StdioAdapter;
use arcs::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use arcs::core::{
    Artifact, SchemaId, SchemaRegistry, SequenceIdGenerator, SourceKind, SystemClock, Trust,
    TrustLevel, VersionId,
};
use arcs::execution::ExecutionService;
use arcs::input::{InputMessage, InputService};
use arcs::request::RequestService;
use arcs::store::{ArtifactRelations, SqliteArtifactStore, relation_kinds};
use arcs::transform::TransformService;
use arcs::world::observation::{ObservationLog, ObservationService};
use arcs::world::{WorldReducer, WorldState};
use serde::Serialize;
use serde_json::{Value, json};

use crate::HostError;
use crate::config::{
    DEFINITION_SCHEMA, HOST_ID, PollingConfig, PreparedAdapter, PreparedConfig,
    TRANSFORM_DEFINITION_SCHEMA,
};

#[derive(Debug, Clone, Serialize)]
pub struct RequestReport {
    pub id: String,
    pub version_id: VersionId,
    pub schema_id: SchemaId,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransformReport {
    pub id: String,
    pub input_version_id: VersionId,
    pub version_id: VersionId,
    pub schema_id: SchemaId,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterReport {
    pub adapter_id: AdapterId,
    pub enabled_capabilities: Vec<CapabilityId>,
    pub observations_received: u64,
    pub observations_committed: u64,
    pub observations_replayed: u64,
    pub poll_limit_hits: u64,
    pub requests: Vec<RequestReport>,
    pub transforms: Vec<TransformReport>,
    pub inputs: Vec<Artifact>,
    pub input_limit_hits: u64,
}

#[derive(Debug, Serialize)]
pub struct EstimateReport {
    pub entity_id: String,
    pub schema_id: SchemaId,
    pub value: Value,
    pub evidence_version: VersionId,
    pub trust: Trust,
}

#[derive(Debug, Serialize)]
pub struct HostReport {
    pub stored_artifacts: u64,
    pub world_revision: u64,
    pub adapters: Vec<AdapterReport>,
    pub estimates: Vec<EstimateReport>,
}

struct RunningAdapter {
    config: PreparedAdapter,
    endpoint: StdioAdapter,
    report: AdapterReport,
}

pub struct Host {
    schemas: SchemaRegistry,
    registry: AdapterRegistry,
    store: SqliteArtifactStore,
    ids: SequenceIdGenerator,
    world: WorldState,
    adapters: Vec<RunningAdapter>,
    polling: PollingConfig,
}

fn invalid(message: impl Into<String>) -> HostError {
    HostError::InvalidConfig(message.into())
}

fn register_host(
    registry: &mut AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    adapters: &[PreparedAdapter],
) -> Result<(), HostError> {
    let mut capabilities = vec![CapabilityDescriptor {
        id: CapabilityId("request.definition".into()),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId(DEFINITION_SCHEMA.into())],
        },
        required_permissions: vec![],
    }];
    for request in adapters.iter().flat_map(|adapter| &adapter.requests) {
        capabilities.push(CapabilityDescriptor {
            id: CapabilityId(format!("request.input.{}", request.id)),
            contract: CapabilityContract::Input {
                emits: vec![request.input_schema_id.clone()],
            },
            required_permissions: vec![],
        });
    }
    capabilities.push(CapabilityDescriptor {
        id: CapabilityId("transform.definition".into()),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId(TRANSFORM_DEFINITION_SCHEMA.into())],
        },
        required_permissions: vec![],
    });
    for transform in adapters.iter().flat_map(|adapter| &adapter.transforms) {
        capabilities.push(CapabilityDescriptor {
            id: CapabilityId(format!("transform.input.{}", transform.id)),
            contract: CapabilityContract::Input {
                emits: vec![transform.input_schema_id.clone()],
            },
            required_permissions: vec![],
        });
    }
    let grant = AdapterGrant {
        adapter_id: AdapterId(HOST_ID.into()),
        producer_class: ProducerClass::System,
        enabled_capabilities: capabilities.iter().map(|c| c.id.clone()).collect(),
        granted_permissions: vec![],
        assigned_trust: TrustLevel::High,
        ingress_source_kind: Some(SourceKind::Internal),
        max_payload_bytes: 65_536,
        max_external_reference_bytes: 2048,
        data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
    };
    registry
        .register(
            AdapterManifest {
                protocol_version: ADAPTER_PROTOCOL_VERSION,
                adapter_id: grant.adapter_id.clone(),
                adapter_version: env!("CARGO_PKG_VERSION").into(),
                capabilities,
            },
            grant,
            schemas,
            store,
        )
        .map_err(HostError::Registration)
}

fn validate_operations(
    registry: &AdapterRegistry,
    config: &PreparedAdapter,
) -> Result<(), HostError> {
    for request in &config.requests {
        let (_, descriptor) = registry
            .authorized_capability(&config.grant.adapter_id, &request.capability_id)
            .map_err(HostError::Registration)?;
        let CapabilityContract::Request { accepts, emits } = &descriptor.contract else {
            return Err(invalid(format!(
                "{} is not a Request capability for request {}",
                request.capability_id.0, request.id
            )));
        };
        if !accepts.contains(&request.input_schema_id)
            || !emits.contains(&request.response_schema_id)
        {
            return Err(invalid(format!(
                "schema contract mismatch for request {}",
                request.id
            )));
        }
    }
    for transform in &config.transforms {
        let (_, descriptor) = registry
            .authorized_capability(&config.grant.adapter_id, &transform.capability_id)
            .map_err(HostError::Registration)?;
        let CapabilityContract::Transform { accepts, emits } = &descriptor.contract else {
            return Err(invalid(format!(
                "{} is not a Transform capability for {}",
                transform.capability_id.0, transform.id
            )));
        };
        if !accepts.contains(&transform.input_schema_id)
            || !emits.contains(&transform.output_schema_id)
        {
            return Err(invalid(format!(
                "schema contract mismatch for transform {}",
                transform.id
            )));
        }
    }
    Ok(())
}

impl Host {
    pub fn reason(
        &mut self,
        request: arcs::reasoning::ReasoningRequestV2,
    ) -> Result<arcs::reasoning::general::ReasoningResult, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == request.capability.adapter_id)
            .ok_or_else(|| invalid("unknown reasoning adapter"))?;
        arcs::reasoning::general::GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
        .reason(&adapter.endpoint, request)
        .map_err(HostError::Reasoning)
    }

    pub fn poll_inputs(&mut self) -> Result<Vec<Artifact>, HostError> {
        let mut received = Vec::new();
        for adapter in &mut self.adapters {
            if !adapter.endpoint.manifest().capabilities.iter().any(|cap| {
                cap.contract.is_input()
                    && adapter.config.grant.enabled_capabilities.contains(&cap.id)
            }) {
                continue;
            }
            let mut idle = false;
            let mut inputs = InputService::new(
                &self.registry,
                &self.schemas,
                &self.store,
                &mut self.ids,
                &SystemClock,
            );
            for _ in 0..self.polling.max_inputs_per_adapter {
                let Some(input) = adapter.endpoint.poll_input(&mut inputs).map_err(|source| {
                    HostError::Adapter {
                        adapter: adapter.config.grant.adapter_id.clone(),
                        source,
                    }
                })?
                else {
                    idle = true;
                    break;
                };
                adapter.report.inputs.push(input.clone());
                received.push(input);
            }
            if !idle {
                adapter.report.input_limit_hits += 1;
            }
        }
        Ok(received)
    }

    pub fn prepare_output(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        result_schema: &SchemaId,
    ) -> Result<Artifact, HostError> {
        arcs::output::OutputService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .prepare(capability, input, result_schema)
        .map_err(|source| HostError::Output {
            adapter: capability.adapter_id.clone(),
            source,
        })
    }

    pub fn deliver_output(
        &mut self,
        adapter_id: &AdapterId,
        request: &VersionId,
        approval: &VersionId,
    ) -> Result<Artifact, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == *adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", adapter_id.0)))?;
        arcs::output::OutputService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .deliver(&adapter.endpoint, request, approval)
        .map_err(|source| HostError::Output {
            adapter: adapter_id.clone(),
            source,
        })
    }

    /// Kopiert referenzierte Daten unter den aktuellen Grants. Schreibt keine
    /// neuen World-Observations und erteilt keine zusätzlichen Berechtigungen.
    pub fn copy_data(
        &self,
        capability: &CapabilityRef,
        reference: &arcs::adapters::data::DataReference,
        writer: &mut impl std::io::Write,
    ) -> Result<u64, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", capability.adapter_id.0)))?;
        arcs::adapters::data::DataService::new(&self.registry)
            .copy(&adapter.endpoint, capability, reference, writer)
            .map_err(HostError::Data)
    }

    /// Lädt Dateien, öffnet den Store, spielt Observations ein und startet die
    /// konfigurierten Prozesse. Führt noch keine Datenabfrage oder Action aus.
    pub fn open(config_path: impl AsRef<Path>) -> Result<Self, HostError> {
        let config = PreparedConfig::load(config_path.as_ref())?;
        if let Some(parent) = config.database.parent() {
            std::fs::create_dir_all(parent).map_err(|source| HostError::Io {
                path: parent.into(),
                source,
            })?;
        }
        let store = SqliteArtifactStore::open(&config.database).map_err(HostError::Store)?;
        let ids = store
            .durable_id_generator(&config.node_id)
            .map_err(HostError::Store)?;
        let mut world = WorldState::new();
        for recorded in ObservationLog::new(&store)
            .after(None)
            .map_err(HostError::Store)?
        {
            WorldReducer::new()
                .reduce(&mut world, &recorded)
                .map_err(HostError::Reduction)?;
        }
        let mut registry = AdapterRegistry::new();
        // Bindet alle Schemas vor dem ersten Prozessstart; Drift bleibt ein
        // sichtbarer Fehler und löst keine externe Operation aus.
        register_host(&mut registry, &config.schemas, &store, &config.adapters)?;
        let mut adapters = Vec::new();
        for prepared in config.adapters {
            let mut command = Command::new(&prepared.program);
            command
                .args(&prepared.args)
                .current_dir(&prepared.working_directory);
            let adapter_id = prepared.grant.adapter_id.clone();
            let endpoint = StdioAdapter::connect(
                &mut command,
                Duration::from_millis(prepared.timeout_ms),
                prepared.grant.clone(),
                &mut registry,
                &config.schemas,
                &store,
            )
            .map_err(|source| HostError::Adapter {
                adapter: adapter_id.clone(),
                source,
            })?;
            validate_operations(&registry, &prepared)?;
            let report = AdapterReport {
                adapter_id,
                enabled_capabilities: prepared.grant.enabled_capabilities.clone(),
                observations_received: 0,
                observations_committed: 0,
                observations_replayed: 0,
                poll_limit_hits: 0,
                requests: vec![],
                transforms: vec![],
                inputs: vec![],
                input_limit_hits: 0,
            };
            adapters.push(RunningAdapter {
                config: prepared,
                endpoint,
                report,
            });
        }
        Ok(Self {
            schemas: config.schemas,
            registry,
            store,
            ids,
            world,
            adapters,
            polling: config.polling,
        })
    }

    pub fn store(&self) -> &SqliteArtifactStore {
        &self.store
    }
    pub fn schemas(&self) -> &SchemaRegistry {
        &self.schemas
    }
    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }
    pub fn world(&self) -> &WorldState {
        &self.world
    }

    fn input(
        &mut self,
        input_id: &str,
        capability: &str,
        subject: &str,
        payload: Value,
    ) -> Result<Artifact, HostError> {
        InputService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(HOST_ID.into()),
            InputMessage {
                capability_id: CapabilityId(capability.into()),
                external_subject: Some(subject.into()),
                external_reference: format!("configured-request:{input_id}"),
                payload,
            },
        )
        .map_err(|source| HostError::Input {
            request: input_id.into(),
            source,
        })
    }

    /// Eine konfigurierte ID bezeichnet eine einzelne, persistente Abfrage.
    /// Änderungen an Ziel oder Eingabe benötigen eine neue Request-ID.
    pub fn request(&mut self, request_id: &str) -> Result<Artifact, HostError> {
        let (index, request) = self
            .adapters
            .iter()
            .enumerate()
            .find_map(|(index, adapter)| {
                adapter
                    .config
                    .requests
                    .iter()
                    .find(|r| r.id == request_id)
                    .cloned()
                    .map(|r| (index, r))
            })
            .ok_or_else(|| invalid(format!("unknown request ID {request_id}")))?;
        let adapter_id = self.adapters[index].config.grant.adapter_id.clone();
        let definition = self.input(
            &request.id,
            "request.definition",
            &request.id,
            json!({"adapter_id": adapter_id, "request": request}),
        )?;
        let input = self.input(
            &request.id,
            &format!("request.input.{}", request.id),
            &request.subject,
            request.payload.clone(),
        )?;
        let relations = ArtifactRelations::new(&self.store);
        if !relations
            .outgoing(&input.version_id)
            .map_err(HostError::Store)?
            .iter()
            .any(|relation| {
                relation.to == definition.version_id
                    && relation.kind == relation_kinds::supported_by()
            })
        {
            relations
                .connect(
                    input.version_id.clone(),
                    definition.version_id,
                    relation_kinds::supported_by(),
                )
                .map_err(HostError::Store)?;
        }
        let result = self
            .request_version(
                &CapabilityRef {
                    adapter_id,
                    capability_id: request.capability_id.clone(),
                },
                &input.version_id,
                &request.response_schema_id,
            )
            .map_err(|error| match error {
                HostError::Request { source, .. } => HostError::Request {
                    request: request.id.clone(),
                    source,
                },
                other => other,
            })?;
        let report = &mut self.adapters[index].report.requests;
        report.retain(|previous| previous.id != request_id);
        report.push(RequestReport {
            id: request.id,
            version_id: result.version_id.clone(),
            schema_id: result.schema_id.clone(),
            payload: result.payload.clone(),
        });
        Ok(result)
    }

    /// Führt eine gezielte Abfrage mit einer bereits persistierten Eingabe aus.
    /// Eine neue Messung benötigt eine neue Eingabeversion; Replay bleibt stabil.
    pub fn request_version(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        response_schema: &SchemaId,
    ) -> Result<Artifact, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", capability.adapter_id.0)))?;
        RequestService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .execute(&adapter.endpoint, capability, input, response_schema)
        .map_err(|source| HostError::Request {
            request: input.0.clone(),
            source,
        })
    }

    /// Verarbeitet ein bereits gespeichertes Artefakt über eine freigegebene
    /// Transform-Capability. Fachliche Adapterpakete werden nicht eingebunden.
    pub fn transform(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        output_schema: &SchemaId,
    ) -> Result<Artifact, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", capability.adapter_id.0)))?;
        TransformService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .execute(&adapter.endpoint, capability, input, output_schema)
        .map_err(|source| HostError::Transform {
            adapter: capability.adapter_id.clone(),
            source,
        })
    }

    /// Eine konfigurierte ID bezeichnet eine dauerhafte Definition. Geänderte
    /// Parameter benötigen eine neue ID, wie bei konfigurierten Requests.
    pub fn transform_configured(&mut self, id: &str) -> Result<Artifact, HostError> {
        let (index, transform) = self
            .adapters
            .iter()
            .enumerate()
            .find_map(|(index, adapter)| {
                adapter
                    .config
                    .transforms
                    .iter()
                    .find(|transform| transform.id == id)
                    .cloned()
                    .map(|transform| (index, transform))
            })
            .ok_or_else(|| invalid(format!("unknown transform ID {id}")))?;
        let adapter_id = self.adapters[index].config.grant.adapter_id.clone();
        let definition = self.input(
            &format!("transform-definition:{id}"),
            "transform.definition",
            id,
            json!({"adapter_id": adapter_id, "transform": transform}),
        )?;
        let input = self.input(
            &format!("transform-input:{id}"),
            &format!("transform.input.{id}"),
            &transform.subject,
            transform.payload.clone(),
        )?;
        let relations = ArtifactRelations::new(&self.store);
        if !relations
            .outgoing(&input.version_id)
            .map_err(HostError::Store)?
            .iter()
            .any(|relation| {
                relation.to == definition.version_id
                    && relation.kind == relation_kinds::supported_by()
            })
        {
            relations
                .connect(
                    input.version_id.clone(),
                    definition.version_id,
                    relation_kinds::supported_by(),
                )
                .map_err(HostError::Store)?;
        }
        let result = self.transform(
            &CapabilityRef {
                adapter_id,
                capability_id: transform.capability_id,
            },
            &input.version_id,
            &transform.output_schema_id,
        )?;
        let report = &mut self.adapters[index].report.transforms;
        report.retain(|previous| previous.id != id);
        report.push(TransformReport {
            id: id.into(),
            input_version_id: input.version_id,
            version_id: result.version_id.clone(),
            schema_id: result.schema_id.clone(),
            payload: result.payload.clone(),
        });
        Ok(result)
    }

    /// Für Anwendungen, die bereits eine geprüfte und freigegebene Action
    /// persistiert haben. Dieser Host erteilt selbst keine Freigaben.
    pub fn execute_approved(
        &mut self,
        adapter_id: &AdapterId,
        action: &VersionId,
    ) -> Result<Artifact, HostError> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.config.grant.adapter_id == *adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", adapter_id.0)))?;
        ExecutionService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
        .execute(&adapter.endpoint, action)
        .map_err(|source| HostError::Execution {
            adapter: adapter_id.clone(),
            source,
        })
    }

    /// Begrenzte Runde mit fairem Budget je Adapter. Ein erreichter Grenzwert
    /// ist sichtbar; weitere Daten bleiben beim Adapter für die nächste Runde.
    pub fn poll_round(&mut self) -> Result<(), HostError> {
        for adapter in &mut self.adapters {
            let observes = adapter
                .endpoint
                .manifest()
                .capabilities
                .iter()
                .any(|capability| {
                    capability.contract.is_observation()
                        && adapter
                            .config
                            .grant
                            .enabled_capabilities
                            .contains(&capability.id)
                });
            if !observes {
                continue;
            }
            let mut idle = false;
            let mut observations = ObservationService::new(
                &self.registry,
                &self.schemas,
                &self.store,
                &mut self.ids,
                &SystemClock,
            );
            for _ in 0..self.polling.max_observations_per_adapter {
                let Some(recorded) =
                    adapter.endpoint.poll(&mut observations).map_err(|source| {
                        HostError::Adapter {
                            adapter: adapter.config.grant.adapter_id.clone(),
                            source,
                        }
                    })?
                else {
                    idle = true;
                    break;
                };
                adapter.report.observations_received += 1;
                if self
                    .world
                    .cursor()
                    .is_none_or(|cursor| recorded.cursor() > cursor)
                {
                    WorldReducer::new()
                        .reduce(&mut self.world, &recorded)
                        .map_err(HostError::Reduction)?;
                    adapter.report.observations_committed += 1;
                } else {
                    adapter.report.observations_replayed += 1;
                }
            }
            if !idle {
                adapter.report.poll_limit_hits += 1;
            }
        }
        Ok(())
    }

    /// Ein Fehler beim Beenden verhindert nicht das Beenden der übrigen Childs.
    pub fn shutdown(&mut self) -> Result<(), HostError> {
        let mut first_error = None;
        for adapter in &mut self.adapters {
            if let Err(source) = adapter.endpoint.shutdown()
                && first_error.is_none()
            {
                first_error = Some(HostError::Adapter {
                    adapter: adapter.config.grant.adapter_id.clone(),
                    source,
                });
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn report(&self) -> Result<HostReport, HostError> {
        Ok(HostReport {
            stored_artifacts: self.store.len().map_err(HostError::Store)?,
            world_revision: self.world.revision().get(),
            adapters: self
                .adapters
                .iter()
                .map(|adapter| adapter.report.clone())
                .collect(),
            estimates: self
                .world
                .estimates()
                .map(|(_, estimate)| EstimateReport {
                    entity_id: estimate.entity_id().as_str().into(),
                    schema_id: estimate.schema_id().clone(),
                    value: estimate.value().clone(),
                    evidence_version: estimate.evidence_version().clone(),
                    trust: estimate.evidence_trust().clone(),
                })
                .collect(),
        })
    }

    /// Führt konfigurierte Abfragen und Transforms einmal und danach die Polling-Runden aus.
    /// Alle Prozesse werden vor Rückgabe beendet, auch bei Fehlern via Drop.
    pub fn run(mut self) -> Result<HostReport, HostError> {
        let requests: Vec<_> = self
            .adapters
            .iter()
            .flat_map(|a| &a.config.requests)
            .map(|r| r.id.clone())
            .collect();
        for id in requests {
            self.request(&id)?;
        }
        let transforms: Vec<_> = self
            .adapters
            .iter()
            .flat_map(|adapter| &adapter.config.transforms)
            .map(|transform| transform.id.clone())
            .collect();
        for id in transforms {
            self.transform_configured(&id)?;
        }
        for round in 0..self.polling.rounds {
            self.poll_inputs()?;
            self.poll_round()?;
            if round + 1 < self.polling.rounds && self.polling.interval_ms > 0 {
                std::thread::sleep(Duration::from_millis(self.polling.interval_ms));
            }
        }
        self.shutdown()?;
        self.report()
    }
}

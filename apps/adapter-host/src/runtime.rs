mod input;
mod reason;
mod reconnect;
mod request;
mod resources;
mod sessions;
pub use reconnect::AdapterReconnectReport;
pub use request::RequestWorkerReport;
pub use resources::ResourcePoolReport;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
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
    pub resource_pools: Vec<ResourcePoolReport>,
    pub request_workers: Vec<RequestWorkerReport>,
}

struct RunningAdapter {
    config: PreparedAdapter,
    endpoint: Arc<StdioAdapter>,
    report: AdapterReport,
}

pub struct Host {
    database_path: std::path::PathBuf,
    schemas: SchemaRegistry,
    registry: AdapterRegistry,
    store: SqliteArtifactStore,
    ids: SequenceIdGenerator,
    world: WorldState,
    adapters: Vec<RunningAdapter>,
    polling: PollingConfig,
    reasoning_jobs: HashMap<String, reason::PendingReasoning>,
    request_jobs: HashMap<String, request::PendingRequest>,
    retiring_request_jobs: Vec<request::RetiringRequest>,
    unavailable_adapters: HashSet<AdapterId>,
    resource_pools: crate::resource_policy::ResourceQuantities,
    reconnect_installations: std::collections::BTreeMap<AdapterId, VersionId>,
    reconnect_states: HashMap<AdapterId, arcs::adapters::reconnect::ConnectionState>,
    reconnect_jobs: HashMap<AdapterId, reconnect::ReconnectJob>,
    reconnect_cursor: Option<AdapterId>,
    reconnect_stopped: bool,
    _reconnect_lock: Option<std::fs::File>,
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
        description: None,
        id: CapabilityId("request.definition".into()),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId(DEFINITION_SCHEMA.into())],
        },
        required_permissions: vec![],
    }];
    for request in adapters.iter().flat_map(|adapter| &adapter.requests) {
        capabilities.push(CapabilityDescriptor {
            description: None,
            id: CapabilityId(format!("request.input.{}", request.id)),
            contract: CapabilityContract::Input {
                emits: vec![request.input_schema_id.clone()],
            },
            required_permissions: vec![],
        });
    }
    capabilities.push(CapabilityDescriptor {
        description: None,
        id: CapabilityId("transform.definition".into()),
        contract: CapabilityContract::Input {
            emits: vec![SchemaId(TRANSFORM_DEFINITION_SCHEMA.into())],
        },
        required_permissions: vec![],
    });
    for transform in adapters.iter().flat_map(|adapter| &adapter.transforms) {
        capabilities.push(CapabilityDescriptor {
            description: None,
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
        input_data_limits: None,
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
    /// Mehrere Implementierungen pro Rolle sind gleichberechtigte Ziele.
    pub fn capabilities(&self, role: arcs::adapters::AdapterRole) -> Vec<CapabilityRef> {
        self.registry
            .capabilities(role)
            .into_iter()
            .filter(|cap| {
                self.adapters
                    .iter()
                    .any(|adapter| adapter.config.grant.adapter_id == cap.adapter_id)
            })
            .collect()
    }

    /// Katalog der angeschlossenen externen Installationen mit ihren vom
    /// Betreiber freigeschalteten Fähigkeiten. Kein Health- oder Wirkungsnachweis.
    pub fn adapter_catalog(&self) -> Vec<arcs::adapters::AdapterConnection> {
        self.registry
            .catalog()
            .into_iter()
            .filter(|connection| {
                self.adapters
                    .iter()
                    .any(|adapter| adapter.config.grant.adapter_id == connection.adapter_id)
            })
            .collect()
    }

    /// Search only connected external installations not already quarantined.
    /// A match is advisory; the actual manager must authorize every later call.
    pub fn find_capabilities(
        &self,
        query: &arcs::adapters::CapabilityQuery,
    ) -> Result<Vec<arcs::adapters::CapabilityMatch>, arcs::adapters::CapabilityDiscoveryError>
    {
        self.registry.find_capabilities_with(query, |capability| {
            !self.unavailable_adapters.contains(&capability.adapter_id)
                && self
                    .adapters
                    .iter()
                    .any(|adapter| adapter.config.grant.adapter_id == capability.adapter_id)
        })
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
        self.ensure_adapter_available(adapter_id)?;
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
        .deliver(adapter.endpoint.as_ref(), request, approval)
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
        self.ensure_adapter_available(&capability.adapter_id)?;
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid(format!("unknown adapter {}", capability.adapter_id.0)))?;
        arcs::adapters::data::DataService::new(&self.registry)
            .copy(adapter.endpoint.as_ref(), capability, reference, writer)
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
        let database_path = config
            .database
            .canonicalize()
            .map_err(|source| HostError::Io {
                path: config.database.clone(),
                source,
            })?;
        let mut ids = store
            .durable_id_generator(&config.node_id)
            .map_err(HostError::Store)?;
        let reconnect_lock = if config
            .adapters
            .iter()
            .any(|adapter| adapter.reasoning_reconnect.is_some())
        {
            Some(reconnect::lock_node(&database_path, &config.node_id)?)
        } else {
            None
        };
        let mut reconnect_installations = std::collections::BTreeMap::new();
        let mut reconnect_states = HashMap::new();
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
            let mut command = prepared.command();
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
            if let Some(policy) = &prepared.reasoning_reconnect {
                let definition = arcs::adapters::reconnect::ReconnectDefinition {
                    capabilities_verified: false,
                    node_id: config.node_id.clone(),
                    manifest: endpoint.manifest().clone(),
                    grant: prepared.grant.clone(),
                    policy: policy.clone(),
                };
                let receipt = arcs::adapters::reconnect::ReconnectService {
                    registry: &registry,
                    schemas: &config.schemas,
                    store: &store,
                    ids: &mut ids,
                    clock: &SystemClock,
                }
                .observe_connected(&definition)
                .map_err(reconnect::error)?;
                reconnect_installations
                    .insert(adapter_id.clone(), receipt.record.definition_version_id);
                reconnect_states.insert(adapter_id.clone(), receipt.record.state);
            }

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
                endpoint: Arc::new(endpoint),
                report,
            });
        }
        Ok(Self {
            database_path,
            schemas: config.schemas,
            registry,
            store,
            ids,
            world,
            adapters,
            polling: config.polling,
            reasoning_jobs: HashMap::new(),
            request_jobs: HashMap::new(),
            retiring_request_jobs: Vec::new(),
            unavailable_adapters: HashSet::new(),
            resource_pools: config.resource_pools,
            reconnect_installations,
            reconnect_states,
            reconnect_jobs: HashMap::new(),
            reconnect_cursor: None,
            reconnect_stopped: false,
            _reconnect_lock: reconnect_lock,
        })
    }

    pub fn store(&self) -> &SqliteArtifactStore {
        &self.store
    }
    pub(crate) fn database_path(&self) -> &Path {
        &self.database_path
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
        self.ensure_adapter_available(&capability.adapter_id)?;
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
        .execute(
            adapter.endpoint.as_ref(),
            capability,
            input,
            response_schema,
        )
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
        self.ensure_adapter_available(&capability.adapter_id)?;
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
        .execute(adapter.endpoint.as_ref(), capability, input, output_schema)
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
        self.ensure_adapter_available(adapter_id)?;
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
        .execute(adapter.endpoint.as_ref(), action)
        .map_err(|source| HostError::Execution {
            adapter: adapter_id.clone(),
            source,
        })
    }

    /// Begrenzte Runde mit fairem Budget je Adapter. Ein erreichter Grenzwert
    /// ist sichtbar; weitere Daten bleiben beim Adapter für die nächste Runde.
    pub fn poll_round(&mut self) -> Result<(), HostError> {
        let busy = self.busy_adapters();
        for adapter in &mut self.adapters {
            if busy.contains(&adapter.config.grant.adapter_id)
                || self
                    .unavailable_adapters
                    .contains(&adapter.config.grant.adapter_id)
            {
                continue;
            }
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
        let reconnect_error = self.cancel_reconnections().err();
        let mut cancelled = self.cancel_reasoning_jobs();
        cancelled.extend(self.cancel_request_jobs());
        let mut first_error = reconnect_error;
        for adapter in &mut self.adapters {
            if cancelled.contains(&adapter.config.grant.adapter_id)
                || self
                    .unavailable_adapters
                    .contains(&adapter.config.grant.adapter_id)
            {
                continue;
            }
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
            resource_pools: self.resource_usage(),
            request_workers: self.request_workers(),
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

    /// Führt die ausdrücklich konfigurierten Abfragen und Transforms aus.
    /// Dauerbetrieb und begrenzter Lauf verwenden dieselben Aufrufe und IDs.
    pub fn run_configured_operations(&mut self) -> Result<(), HostError> {
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
        Ok(())
    }

    /// Führt konfigurierte Abfragen und Transforms einmal und danach die Polling-Runden aus.
    /// Alle Prozesse werden vor Rückgabe beendet, auch bei Fehlern via Drop.
    pub fn run(mut self) -> Result<HostReport, HostError> {
        self.run_configured_operations()?;
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

impl Drop for Host {
    fn drop(&mut self) {
        // Keep node ownership until all old consumers have stopped. A fresh
        // host must not overlap a reader retained by an abandoned host object.
        let _ = self.cancel_reconnections();
        self.cancel_reasoning_jobs();
        self.cancel_request_jobs();
    }
}

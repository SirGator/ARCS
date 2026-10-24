use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use arcs::adapters::{AdapterGrant, CapabilityId};
use arcs::core::{MAX_SOURCE_REFERENCE_BYTES, SchemaId, SchemaRegistry};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::HostError;

pub(crate) const HOST_ID: &str = "arcs.host";
pub(crate) const PIPELINE_ID: &str = "arcs.host.pipeline";
pub(crate) const RUNTIME_ID: &str = "arcs.host.runtime";
pub(crate) const DEFINITION_SCHEMA: &str = "arcs.input.host_request.v1";
pub(crate) const TRANSFORM_DEFINITION_SCHEMA: &str = "arcs.input.host_transform.v1";

fn default_node() -> String {
    "adapter-host".into()
}
fn default_timeout() -> u64 {
    5000
}
fn one() -> u32 {
    1
}
fn default_batch() -> u32 {
    1000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    pub config_version: u32,
    pub database: PathBuf,
    #[serde(default = "default_node")]
    pub node_id: String,
    #[serde(default)]
    pub schemas: Vec<PathBuf>,
    pub adapters: Vec<AdapterConfig>,
    #[serde(default)]
    pub polling: PollingConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterConfig {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<Argument>,
    pub profile: Option<PathBuf>,
    pub working_directory: Option<PathBuf>,
    pub grant_file: PathBuf,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub requests: Vec<RequestConfig>,
    #[serde(default)]
    pub transforms: Vec<TransformConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Literal(String),
    File(PathArgument),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathArgument {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollingConfig {
    #[serde(default = "one")]
    pub rounds: u32,
    #[serde(default)]
    pub interval_ms: u64,
    #[serde(default = "default_batch")]
    pub max_observations_per_adapter: u32,
    #[serde(default = "default_batch")]
    pub max_inputs_per_adapter: u32,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            rounds: 1,
            interval_ms: 0,
            max_observations_per_adapter: 1000,
            max_inputs_per_adapter: 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestConfig {
    pub id: String,
    pub capability_id: CapabilityId,
    pub input_schema_id: SchemaId,
    pub response_schema_id: SchemaId,
    pub subject: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformConfig {
    pub id: String,
    pub capability_id: CapabilityId,
    pub input_schema_id: SchemaId,
    pub output_schema_id: SchemaId,
    pub subject: String,
    pub payload: Value,
}

pub(crate) struct PreparedConfig {
    pub database: PathBuf,
    pub node_id: String,
    pub schemas: SchemaRegistry,
    pub adapters: Vec<PreparedAdapter>,
    pub polling: PollingConfig,
}

pub(crate) struct PreparedAdapter {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub working_directory: PathBuf,
    pub grant: AdapterGrant,
    pub timeout_ms: u64,
    pub requests: Vec<RequestConfig>,
    pub transforms: Vec<TransformConfig>,
}

fn invalid(message: impl Into<String>) -> HostError {
    HostError::InvalidConfig(message.into())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, HostError> {
    let bytes = std::fs::read(path).map_err(|source| HostError::Io {
        path: path.into(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| HostError::Json {
        path: path.into(),
        source,
    })
}

fn resolve(base: &Path, path: &Path) -> Result<PathBuf, HostError> {
    if path.as_os_str().is_empty() {
        return Err(invalid("empty filesystem path"));
    }
    Ok(base.join(path))
}

fn require_file(path: &Path) -> Result<(), HostError> {
    let metadata = std::fs::metadata(path).map_err(|source| HostError::Io {
        path: path.into(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(invalid(format!("{} is not a file", path.display())));
    }
    Ok(())
}

fn identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

impl PreparedConfig {
    /// Dateien und statische Konfiguration vor Datenbank und Childs prüfen.
    pub fn load(path: &Path) -> Result<Self, HostError> {
        let path = path.canonicalize().map_err(|source| HostError::Io {
            path: path.into(),
            source,
        })?;
        let base = path.parent().expect("canonical config has a parent");
        let config: HostConfig = read_json(&path)?;
        if config.config_version != 1 {
            return Err(invalid("supported config_version is 1"));
        }
        if !identifier(&config.node_id, 64) {
            return Err(invalid(
                "node_id must contain 1..64 ASCII letters, digits, '.', '_' or '-'",
            ));
        }
        if config.adapters.is_empty() {
            return Err(invalid("at least one adapter is required"));
        }
        if config.polling.rounds == 0
            || config.polling.max_observations_per_adapter == 0
            || config.polling.max_inputs_per_adapter == 0
            || config.polling.interval_ms > 60_000
        {
            return Err(invalid(
                "polling rounds and batch size must be positive; interval_ms must be <= 60000",
            ));
        }
        let database = resolve(base, &config.database)?;
        let mut schemas =
            SchemaRegistry::with_bundled_schemas().map_err(|source| HostError::Schema {
                path: "<bundled>".into(),
                source,
            })?;
        schemas
            .register_json(include_str!("../schemas/request-definition.schema.json"))
            .map_err(|source| HostError::Schema {
                path: "<host-request>".into(),
                source,
            })?;
        schemas
            .register_json(include_str!("../schemas/transform-definition.schema.json"))
            .map_err(|source| HostError::Schema {
                path: "<host-transform>".into(),
                source,
            })?;
        schemas
            .register_json(include_str!("../schemas/pipeline-record.schema.json"))
            .map_err(|source| HostError::Schema {
                path: "<host-pipeline>".into(),
                source,
            })?;
        schemas
            .register_json(include_str!("../schemas/runtime-definition.schema.json"))
            .map_err(|source| HostError::Schema {
                path: "<host-runtime>".into(),
                source,
            })?;
        for path in config.schemas {
            let path = resolve(base, &path)?;
            let document = std::fs::read_to_string(&path).map_err(|source| HostError::Io {
                path: path.clone(),
                source,
            })?;
            schemas
                .register_json(&document)
                .map_err(|source| HostError::Schema { path, source })?;
        }
        let mut adapter_ids = HashSet::new();
        let mut request_ids = HashSet::new();
        let mut adapters = Vec::new();
        for adapter in config.adapters {
            if !(1..=3_600_000).contains(&adapter.timeout_ms) {
                return Err(invalid("timeout_ms must be 1..3600000"));
            }
            let program = resolve(base, &adapter.program)?;
            require_file(&program)?;
            let working_directory = resolve(
                base,
                adapter
                    .working_directory
                    .as_deref()
                    .unwrap_or(Path::new(".")),
            )?;
            if !working_directory.is_dir() {
                return Err(invalid(format!(
                    "{} is not a directory",
                    working_directory.display()
                )));
            }
            let mut args = Vec::new();
            if let Some(profile) = adapter.profile {
                let profile = resolve(base, &profile)?;
                require_file(&profile)?;
                args.push(profile.into_os_string());
            }
            for argument in adapter.args {
                args.push(match argument {
                    Argument::Literal(value) => value.into(),
                    Argument::File(value) => resolve(base, &value.path)?.into_os_string(),
                });
            }
            let grant: AdapterGrant = read_json(&resolve(base, &adapter.grant_file)?)?;
            if [HOST_ID, PIPELINE_ID, RUNTIME_ID].contains(&grant.adapter_id.0.as_str())
                || !adapter_ids.insert(grant.adapter_id.clone())
            {
                return Err(invalid(format!(
                    "duplicate or reserved adapter ID {}",
                    grant.adapter_id.0
                )));
            }
            let capabilities: HashSet<_> = grant.enabled_capabilities.iter().collect();
            let permissions: HashSet<_> = grant.granted_permissions.iter().collect();
            if capabilities.is_empty()
                || capabilities.len() != grant.enabled_capabilities.len()
                || permissions.len() != grant.granted_permissions.len()
                || grant.max_payload_bytes == 0
                || !(1..=MAX_SOURCE_REFERENCE_BYTES).contains(&grant.max_external_reference_bytes)
            {
                return Err(invalid(format!("invalid grant for {}", grant.adapter_id.0)));
            }
            if let Some(limits) = grant.data_limits
                && (!limits.is_valid()
                    || !grant
                        .granted_permissions
                        .iter()
                        .any(|permission| permission == "data.read"))
            {
                return Err(invalid(format!(
                    "invalid data limits or missing data.read permission for {}",
                    grant.adapter_id.0
                )));
            }
            if let Some(limits) = &grant.input_data_limits
                && (!limits.is_valid()
                    || !grant
                        .granted_permissions
                        .iter()
                        .any(|permission| permission == "data.receive"))
            {
                return Err(invalid(
                    "invalid input data limits or missing data.receive permission",
                ));
            }
            for request in &adapter.requests {
                if !identifier(&request.id, 64) || !request_ids.insert(request.id.clone()) {
                    return Err(invalid(
                        "request IDs must be globally unique and contain 1..64 ASCII letters, digits, '.', '_' or '-'",
                    ));
                }
                if request.subject.trim().is_empty() || request.subject.len() > 256 {
                    return Err(invalid(format!(
                        "request {} needs a subject of 1..256 bytes",
                        request.id
                    )));
                }
                if !capabilities.contains(&request.capability_id) {
                    return Err(invalid(format!(
                        "request {} capability is not enabled by the grant",
                        request.id
                    )));
                }
                if schemas.get(&request.response_schema_id).is_none() {
                    return Err(invalid(format!(
                        "unknown response schema for request {}",
                        request.id
                    )));
                }
                schemas
                    .validate(&request.input_schema_id, &request.payload)
                    .map_err(|violations| {
                        invalid(format!("request {} payload: {violations:?}", request.id))
                    })?;
                if serde_json::to_vec(&request.payload)
                    .map_err(|source| HostError::Json {
                        path: path.clone(),
                        source,
                    })?
                    .len()
                    > 60 * 1024
                {
                    return Err(invalid(format!(
                        "request {} payload exceeds 60 KiB",
                        request.id
                    )));
                }
            }
            for transform in &adapter.transforms {
                if !identifier(&transform.id, 64) || !request_ids.insert(transform.id.clone()) {
                    return Err(invalid(
                        "request and transform IDs must be globally unique and use 1..64 ASCII letters, digits, '.', '_' or '-'",
                    ));
                }
                if transform.subject.trim().is_empty()
                    || transform.subject.len() > 256
                    || transform.subject.chars().any(char::is_control)
                {
                    return Err(invalid(format!(
                        "invalid subject for transform {}",
                        transform.id
                    )));
                }
                if !capabilities.contains(&transform.capability_id) {
                    return Err(invalid(format!(
                        "transform {} capability is not enabled",
                        transform.id
                    )));
                }
                if schemas.get(&transform.output_schema_id).is_none() {
                    return Err(invalid(format!(
                        "unknown output schema for transform {}",
                        transform.id
                    )));
                }
                schemas
                    .validate(&transform.input_schema_id, &transform.payload)
                    .map_err(|violations| {
                        invalid(format!(
                            "transform {} payload: {violations:?}",
                            transform.id
                        ))
                    })?;
                if serde_json::to_vec(&transform.payload)
                    .map_err(|source| HostError::Json {
                        path: path.clone(),
                        source,
                    })?
                    .len()
                    > 60 * 1024
                {
                    return Err(invalid(format!(
                        "transform {} input exceeds 60 KiB",
                        transform.id
                    )));
                }
            }
            adapters.push(PreparedAdapter {
                program,
                args,
                working_directory,
                grant,
                timeout_ms: adapter.timeout_ms,
                requests: adapter.requests,
                transforms: adapter.transforms,
            });
        }
        Ok(Self {
            database,
            node_id: config.node_id,
            schemas,
            adapters,
            polling: config.polling,
        })
    }
}

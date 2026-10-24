use std::fmt;
use std::path::PathBuf;

use arcs::adapters::stdio::StdioError;
use arcs::adapters::{AdapterId, AdapterRegistryError};
use arcs::core::RegistryError;
use arcs::execution::ExecutionError;
use arcs::input::InputError;
use arcs::request::RequestError;
use arcs::store::StoreError;
use arcs::world::ReduceError;

#[derive(Debug)]
pub enum HostError {
    Reasoning(arcs::reasoning::ReasoningError),
    Output {
        adapter: AdapterId,
        source: arcs::output::OutputError,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Schema {
        path: PathBuf,
        source: RegistryError,
    },
    InvalidConfig(String),
    Store(StoreError),
    Registration(AdapterRegistryError),
    Adapter {
        adapter: AdapterId,
        source: StdioError,
    },
    Input {
        request: String,
        source: InputError,
    },
    Request {
        request: String,
        source: RequestError,
    },
    Execution {
        adapter: AdapterId,
        source: ExecutionError,
    },
    Reduction(ReduceError),
    Data(arcs::adapters::data::DataError),
    Transform {
        adapter: AdapterId,
        source: arcs::transform::TransformError,
    },
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reasoning(source) => write!(f, "reasoning: {source:?}"),
            Self::Output { adapter, source } => {
                write!(f, "adapter {} output: {source:?}", adapter.0)
            }
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Json { path, source } => {
                write!(f, "invalid JSON in {}: {source}", path.display())
            }
            Self::Schema { path, source } => {
                write!(f, "invalid schema {}: {source:?}", path.display())
            }
            Self::InvalidConfig(message) => write!(f, "invalid configuration: {message}"),
            Self::Store(source) => write!(f, "store: {source:?}"),
            Self::Registration(source) => write!(f, "registration: {source:?}"),
            Self::Adapter { adapter, source } => write!(f, "adapter {}: {source:?}", adapter.0),
            Self::Input { request, source } => write!(f, "request {request} input: {source:?}"),
            Self::Request { request, source } => write!(f, "request {request}: {source:?}"),
            Self::Execution { adapter, source } => {
                write!(f, "adapter {} execution: {source:?}", adapter.0)
            }
            Self::Reduction(source) => write!(f, "world-state: {source:?}"),
            Self::Data(source) => write!(f, "{source}"),
            Self::Transform { adapter, source } => {
                write!(f, "adapter {} transform: {source:?}", adapter.0)
            }
        }
    }
}

impl std::error::Error for HostError {}

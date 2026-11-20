//! Konfigurierbarer Host ohne Abhängigkeit von konkreten Geräteadaptern.

pub mod config;
mod error;
pub mod memory;
pub mod monitor;
pub mod operator_command;
pub mod pipeline;
mod process_environment;
pub mod resource_policy;
mod runtime;
pub mod task_runtime;

pub use error::HostError;
pub use runtime::{
    AdapterReconnectReport, AdapterReport, EstimateReport, ExecutionWorkerReport, Host, HostReport,
    RequestReport, RequestWorkerReport, ResourcePoolReport, TransformReport,
};

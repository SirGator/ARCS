//! Konfigurierbarer Host ohne Abhängigkeit von konkreten Geräteadaptern.

pub mod config;
mod error;
pub mod monitor;
pub mod pipeline;
mod process_environment;
pub mod resource_policy;
mod runtime;
pub mod task_runtime;

pub use error::HostError;
pub use runtime::{
    AdapterReport, EstimateReport, Host, HostReport, RequestReport, ResourcePoolReport,
    TransformReport,
};

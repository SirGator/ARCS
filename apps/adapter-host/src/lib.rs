//! Konfigurierbarer Host ohne Abhängigkeit von konkreten Geräteadaptern.

pub mod config;
mod error;
pub mod monitor;
pub mod pipeline;
mod runtime;
pub mod task_runtime;

pub use error::HostError;
pub use runtime::{
    AdapterReport, EstimateReport, Host, HostReport, RequestReport, TransformReport,
};

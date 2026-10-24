//! Konfigurierbarer Host ohne Abhängigkeit von konkreten Geräteadaptern.

pub mod config;
mod error;
pub mod pipeline;
mod runtime;

pub use error::HostError;
pub use runtime::{
    AdapterReport, EstimateReport, Host, HostReport, RequestReport, TransformReport,
};

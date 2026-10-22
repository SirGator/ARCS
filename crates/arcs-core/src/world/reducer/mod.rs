//! Zustandsfunktion `b_t = B(b_{t-1}, o_t)` des ersten World Models.

mod error;
mod service;

pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;

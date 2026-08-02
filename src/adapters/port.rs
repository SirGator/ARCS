//! Gemeinsame Transportfehler externer Adapter-Ports.

/// Transport- oder Protokollfehler eines externen Adapterprozesses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterCallError {
    Unavailable(String),
    Timeout,
    Rejected(String),
    Protocol(String),
}

//! Core receipts for bounded reconnection of a pure Reason installation.
//! Transport workers may handshake; only the owning Core changes session state.
mod records;
#[cfg(test)]
mod tests;
use super::{AdapterGrant, AdapterManifest, AdapterRegistry};
use crate::core::*;
use crate::store::{SqliteArtifactStore, StoreError};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "arcs.adapter_reconnect.v1";
pub const STATE_SCHEMA: &str = "arcs.adapter_reconnect_state.v1";
pub const PUBLISHER: &str = "arcs.adapters.reconnect";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectPolicy {
    pub max_attempts: u8,
    pub delay_ms: u64,
    pub handshake_timeout_ms: u64,
}
impl ReconnectPolicy {
    pub fn validate(&self) -> Result<(), ReconnectError> {
        if !(1..=8).contains(&self.max_attempts)
            || !(1..=60_000).contains(&self.delay_ms)
            || !(1..=3_600_000).contains(&self.handshake_timeout_ms)
        {
            return Err(invalid(
                "reconnect needs 1..8 attempts, a delay of 1..60000 ms and handshake timeout of 1..3600000 ms",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectDefinition {
    pub capabilities_verified: bool,
    pub node_id: String,
    pub manifest: AdapterManifest,
    pub grant: AdapterGrant,
    pub policy: ReconnectPolicy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Healthy,
    Waiting,
    Connecting,
    Exhausted,
    Rejected,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionFailure {
    Interrupted,
    Timeout,
    Transport,
    Contract,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectState {
    pub definition_version_id: VersionId,
    pub generation: u64,
    pub attempt: u8,
    pub state: ConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ConnectionFailure>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ReconnectReceipt {
    pub version_id: VersionId,
    pub record: ReconnectState,
}
#[derive(Debug)]
pub enum ReconnectError {
    Store(StoreError),
    Invalid(String),
    Conflict,
}
impl From<StoreError> for ReconnectError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
fn invalid(message: &str) -> ReconnectError {
    ReconnectError::Invalid(message.into())
}

pub struct ReconnectService<'a> {
    pub registry: &'a AdapterRegistry,
    pub schemas: &'a SchemaRegistry,
    pub store: &'a SqliteArtifactStore,
    pub ids: &'a mut dyn ArtifactIdGenerator,
    pub clock: &'a dyn Clock,
}
impl ReconnectService<'_> {
    /// Called only after an actual successful startup handshake. A prior failed
    /// episode remains in history; request attempts and approvals never change.
    pub fn observe_connected(
        &mut self,
        definition: &ReconnectDefinition,
    ) -> Result<ReconnectReceipt, ReconnectError> {
        records::validate_definition(self.registry, definition)?;
        let artifact = records::persist_definition(self, definition)?;
        let previous = records::load_state(self, &artifact.version_id)?;
        if let Some((_, receipt)) = &previous
            && receipt.record.state == ConnectionState::Healthy
        {
            return Ok(receipt.clone());
        }
        let record = ReconnectState {
            definition_version_id: artifact.version_id,
            generation: previous.as_ref().map_or(0, |(_, r)| r.record.generation),
            attempt: previous.as_ref().map_or(0, |(_, r)| r.record.attempt),
            state: ConnectionState::Healthy,
            not_before_unix_ms: None,
            failure: None,
        };
        records::update(self, &record, previous.as_ref().map(|(a, _)| a))
    }
    pub fn current(&self, definition: &VersionId) -> Result<ReconnectReceipt, ReconnectError> {
        records::load_state(self, definition)?
            .map(|(_, r)| r)
            .ok_or_else(|| invalid("missing reconnect state"))
    }
    pub fn definition(&self, version: &VersionId) -> Result<ReconnectDefinition, ReconnectError> {
        records::load_definition(self, version)
    }
    pub fn note_loss(
        &self,
        definition: &VersionId,
        now: u64,
    ) -> Result<ReconnectReceipt, ReconnectError> {
        let spec = self.definition(definition)?;
        let (previous, receipt) = records::load_state(self, definition)?
            .ok_or_else(|| invalid("missing connected installation"))?;
        if receipt.record.state != ConnectionState::Healthy {
            return Ok(receipt);
        }
        let generation = receipt
            .record
            .generation
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or_else(|| invalid("reconnect generation overflow"))?;
        records::update(
            self,
            &ReconnectState {
                definition_version_id: definition.clone(),
                generation,
                attempt: 0,
                state: ConnectionState::Waiting,
                not_before_unix_ms: Some(due(now, spec.policy.delay_ms)?),
                failure: None,
            },
            Some(&previous),
        )
    }
    pub fn claim(
        &self,
        definition: &VersionId,
        expected: &VersionId,
        now: u64,
    ) -> Result<ReconnectReceipt, ReconnectError> {
        let spec = self.definition(definition)?;
        let (previous, receipt) = records::load_state(self, definition)?
            .ok_or_else(|| invalid("missing reconnect state"))?;
        if receipt.version_id != *expected {
            return Err(ReconnectError::Conflict);
        }
        let mut record = receipt.record;
        if record.state != ConnectionState::Waiting
            || record.attempt >= spec.policy.max_attempts
            || record.not_before_unix_ms.is_none_or(|due| now < due)
            || now > i64::MAX as u64
        {
            return Err(invalid("reconnect attempt is not due"));
        }
        record.state = ConnectionState::Connecting;
        record.attempt += 1;
        record.not_before_unix_ms = None;
        record.failure = None;
        records::update(self, &record, Some(&previous))
    }
    pub fn finish(
        &self,
        definition: &VersionId,
        expected: &VersionId,
        failure: Option<ConnectionFailure>,
        now: u64,
    ) -> Result<ReconnectReceipt, ReconnectError> {
        let spec = self.definition(definition)?;
        let (previous, receipt) = records::load_state(self, definition)?
            .ok_or_else(|| invalid("missing reconnect state"))?;
        if receipt.version_id != *expected {
            return Err(ReconnectError::Conflict);
        }
        let mut record = receipt.record;
        if record.state != ConnectionState::Connecting {
            return Err(invalid("reconnect completion has no claimed attempt"));
        }
        record.failure = failure;
        record.state = match failure {
            None => ConnectionState::Healthy,
            Some(ConnectionFailure::Contract) => ConnectionState::Rejected,
            Some(_) if record.attempt == spec.policy.max_attempts => ConnectionState::Exhausted,
            Some(_) => ConnectionState::Waiting,
        };
        record.not_before_unix_ms = if record.state == ConnectionState::Waiting {
            Some(due(now, spec.policy.delay_ms)?)
        } else {
            None
        };
        records::update(self, &record, Some(&previous))
    }
}
fn due(now: u64, delay: u64) -> Result<u64, ReconnectError> {
    now.checked_add(delay)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| invalid("reconnect delay overflow"))
}

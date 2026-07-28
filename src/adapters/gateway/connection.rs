//! Authentifizierte Brücke von Adapter-Sessions in die Runtime.

use crate::adapters::{AdapterSession, ObservationMessage};
use crate::core::Artifact;
use crate::runtime::{ObservationIngress, ObservationIngressError};

use super::AdapterGateway;

#[derive(Debug)]
pub enum AdapterConnectionError {
    InvalidAdapterSession,
    ObservationIngress(ObservationIngressError),
}

impl From<ObservationIngressError> for AdapterConnectionError {
    fn from(value: ObservationIngressError) -> Self {
        Self::ObservationIngress(value)
    }
}

impl AdapterGateway<'_> {
    /// Authentifiziert die Session und delegiert danach sämtliche fachlichen
    /// Observation-Schritte an den Runtime-Ingress.
    pub fn ingest_observation(
        &mut self,
        session: &AdapterSession,
        message: ObservationMessage,
    ) -> Result<Artifact, AdapterConnectionError> {
        if session.gateway_instance_id != self.instance_id {
            return Err(AdapterConnectionError::InvalidAdapterSession);
        }
        let adapter_id = self
            .adapter_sessions
            .get(&session.token)
            .cloned()
            .ok_or(AdapterConnectionError::InvalidAdapterSession)?;

        let mut ingress = ObservationIngress::new(
            &self.registry,
            self.schemas,
            self.store,
            self.ids.as_mut(),
            self.clock.as_ref(),
        );
        ingress.ingest(&adapter_id, message).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests;

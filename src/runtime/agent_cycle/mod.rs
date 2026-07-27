//! Explizite Phasen eines einzelnen ARCS-Agentenzyklus.
//!
//! Dieser Slice ist bewusst keine domänenspezifische `run`-Methode. Ein Host
//! entscheidet von außen, welche Artifacte aktiv sind, welche Schwelle gilt
//! und welche Adapterfähigkeit zu welchem Schema gehört. Der Core stellt dafür
//! nur die kleinen, kontrollierten Übergänge bereit:
//!
//! 1. persistierte Netzkanten auswerten,
//! 2. eine aktivierte Datenanforderung korreliert erfüllen,
//! 3. einen unbekannten Fall mit kuratiertem Reasoning auflösen,
//! 4. genau einen validierten Vorschlag persistieren,
//! 5. das persistierte Ergebnis korreliert ausliefern.
//!
//! Es gibt hier weder rekursive Propagation noch einen dauerhaften
//! Aktivierungszustand, Lernen, STDP oder implizite externe Aktionen.

use crate::adapters::{
    AdapterGateway, AdapterGatewayError, CapabilityRef, ReasoningRequest, ValidatedProposal,
};
use crate::core::{Artifact, SchemaId, VersionId};
use crate::store::{ActivatedArtifact, ActiveSource, ArtifactNetwork, NetworkError};

use super::routing::{
    HybridRouter, HybridRoutingError, KnownRoutePolicy, KnownRoutePolicyError, RouteResolution,
};

/// Einheitlicher Fehler der Runtime-Fassade.
///
/// Netzwerkfehler und kontrollierte Adaptergrenzen bleiben unterscheidbar;
/// insbesondere wird ein technischer Fehler nie als „unbekannte Situation“
/// umgedeutet und dadurch versehentlich an ein Modell weitergereicht.
#[derive(Debug)]
pub enum AgentCycleError {
    InvalidRoutePolicy(KnownRoutePolicyError),
    Network(NetworkError),
    Adapter(AdapterGatewayError),
}

impl From<NetworkError> for AgentCycleError {
    fn from(value: NetworkError) -> Self {
        Self::Network(value)
    }
}

impl From<AdapterGatewayError> for AgentCycleError {
    fn from(value: AdapterGatewayError) -> Self {
        Self::Adapter(value)
    }
}

impl From<HybridRoutingError> for AgentCycleError {
    fn from(value: HybridRoutingError) -> Self {
        match value {
            HybridRoutingError::Policy(error) => Self::InvalidRoutePolicy(error),
            HybridRoutingError::Network(error) => Self::Network(error),
            HybridRoutingError::Adapter(error) => Self::Adapter(error),
        }
    }
}

/// Kleine produktive Fassade über genau einen vom Host gesteuerten Zyklus.
///
/// Der Host behält die Orchestrierungsentscheidung. Dadurch kann derselbe Core
/// von Server-, Robotik-, UI- oder Software-Adaptern spezialisiert werden,
/// ohne deren Fachbegriffe in die Runtime einzubauen.
pub struct AgentCycle<'gateway, 'resources> {
    gateway: &'gateway mut AdapterGateway<'resources>,
}

impl<'gateway, 'resources> AgentCycle<'gateway, 'resources> {
    pub fn new(gateway: &'gateway mut AdapterGateway<'resources>) -> Self {
        Self { gateway }
    }

    /// Phase 1: Aggregiert direkte Quellbeiträge und wendet die Schwelle an.
    ///
    /// Aktivierungen sind nur das flüchtige Ergebnis dieses Aufrufs. Sie
    /// verändern weder Artifacte noch Current-State-Zeiger.
    pub fn evaluate_network(
        &self,
        sources: &[ActiveSource],
        threshold: f64,
    ) -> Result<Vec<ActivatedArtifact>, AgentCycleError> {
        Ok(ArtifactNetwork::new(self.gateway.store()).propagate_many(sources, threshold)?)
    }

    /// Phase 2: Erfüllt ein zuvor persistiertes Request-Artifact über einen
    /// exakt bezeichneten Data-Port.
    ///
    /// Der Gateway setzt Current-State sowie `fulfills` und `caused_by`
    /// atomar; der Adapter selbst darf diese Metadaten nicht bestimmen.
    pub fn acquire_data(
        &mut self,
        capability: &CapabilityRef,
        request_version: &VersionId,
        response_schema: &SchemaId,
    ) -> Result<Artifact, AgentCycleError> {
        Ok(self
            .gateway
            .request_data(capability, request_version, response_schema)?)
    }

    /// Phase 3: Verwendet zuerst das bekannte Netz und nur bei fehlender,
    /// policy-konformer Route den explizit kuratierten ReasoningRequest.
    pub fn resolve_with_fallback(
        &mut self,
        sources: &[ActiveSource],
        threshold: f64,
        known_route_policy: &KnownRoutePolicy,
        reasoning_request: ReasoningRequest,
    ) -> Result<RouteResolution, AgentCycleError> {
        let mut router = HybridRouter::new(&mut *self.gateway);
        Ok(router.resolve(sources, threshold, known_route_policy, reasoning_request)?)
    }

    /// Phase 4: Persistiert einen bereits vollständig validierten
    /// Modellvorschlag als niedrig vertrauenswürdiges Candidate-Artifact.
    ///
    /// Dies ist ausdrücklich keine Aktionsfreigabe.
    pub fn commit_proposal(
        &mut self,
        proposal: ValidatedProposal,
    ) -> Result<Artifact, AgentCycleError> {
        Ok(self.gateway.commit_proposal(proposal)?)
    }

    /// Phase 5: Liefert eine konkrete persistierte Version über eine
    /// freigeschaltete, idempotente Output-Capability aus und speichert das
    /// korrelierte Result-Artifact.
    pub fn deliver_output(
        &mut self,
        capability: &CapabilityRef,
        artifact_version: &VersionId,
        result_schema: &SchemaId,
    ) -> Result<Artifact, AgentCycleError> {
        Ok(self
            .gateway
            .deliver_output(capability, artifact_version, result_schema)?)
    }
}

#[cfg(test)]
mod tests;

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
//! 5. das persistierte Ergebnis später über den eigenständigen Output-Slice
//!    ausliefern.
//!
//! Es gibt hier weder rekursive Propagation noch einen dauerhaften
//! Aktivierungszustand, Lernen, STDP oder implizite externe Aktionen.

use crate::adapters::CapabilityRef;
use crate::core::{Artifact, SchemaId, VersionId};
use crate::reasoning::{ReasoningError, ReasoningRequest, ReasoningService, ValidatedProposal};
use crate::request::{RequestAdapter, RequestError, RequestService};
use crate::store::{
    ActivatedArtifact, ActiveSource, ArtifactNetwork, NetworkError, SqliteArtifactStore,
};

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
    Request(RequestError),
    Reasoning(ReasoningError),
}

impl From<NetworkError> for AgentCycleError {
    fn from(value: NetworkError) -> Self {
        Self::Network(value)
    }
}

impl From<RequestError> for AgentCycleError {
    fn from(value: RequestError) -> Self {
        Self::Request(value)
    }
}

impl From<ReasoningError> for AgentCycleError {
    fn from(value: ReasoningError) -> Self {
        Self::Reasoning(value)
    }
}

impl From<HybridRoutingError> for AgentCycleError {
    fn from(value: HybridRoutingError) -> Self {
        match value {
            HybridRoutingError::Policy(error) => Self::InvalidRoutePolicy(error),
            HybridRoutingError::Network(error) => Self::Network(error),
            HybridRoutingError::Reasoning(error) => Self::Reasoning(error),
        }
    }
}

/// Kleine produktive Fassade über genau einen vom Host gesteuerten Zyklus.
///
/// Der Host behält die Orchestrierungsentscheidung. Dadurch kann derselbe Core
/// von Server-, Robotik-, UI- oder Software-Adaptern spezialisiert werden,
/// ohne deren Fachbegriffe in die Runtime einzubauen.
pub struct AgentCycle<'a> {
    store: &'a SqliteArtifactStore,
}

impl<'a> AgentCycle<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
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
        Ok(ArtifactNetwork::new(self.store).propagate_many(sources, threshold)?)
    }

    /// Phase 2: Erfüllt ein zuvor persistiertes Request-Artifact über einen
    /// exakt bezeichneten Data-Port.
    ///
    /// Der Request-Slice setzt Current-State sowie `fulfills` und `caused_by`
    /// atomar; der Microservice darf diese Metadaten nicht bestimmen.
    pub fn acquire_data(
        &self,
        request_service: &mut RequestService<'_>,
        endpoint: &dyn RequestAdapter,
        capability: &CapabilityRef,
        request_version: &VersionId,
        response_schema: &SchemaId,
    ) -> Result<Artifact, AgentCycleError> {
        Ok(request_service.execute(endpoint, capability, request_version, response_schema)?)
    }

    /// Phase 3: Verwendet zuerst das bekannte Netz und nur bei fehlender,
    /// policy-konformer Route den explizit kuratierten ReasoningRequest.
    pub fn resolve_with_fallback(
        &self,
        reasoning: &mut ReasoningService<'_>,
        sources: &[ActiveSource],
        threshold: f64,
        known_route_policy: &KnownRoutePolicy,
        reasoning_request: ReasoningRequest,
    ) -> Result<RouteResolution, AgentCycleError> {
        let mut router = HybridRouter::new(self.store, reasoning);
        Ok(router.resolve(sources, threshold, known_route_policy, reasoning_request)?)
    }

    /// Phase 4: Persistiert einen bereits vollständig validierten
    /// Modellvorschlag als niedrig vertrauenswürdiges Candidate-Artifact.
    ///
    /// Dies ist ausdrücklich keine Aktionsfreigabe.
    pub fn commit_proposal(
        &self,
        reasoning: &mut ReasoningService<'_>,
        proposal: ValidatedProposal,
    ) -> Result<Artifact, AgentCycleError> {
        Ok(reasoning.commit_proposal(proposal)?)
    }
}

//! Kleine Orchestrierung zwischen deterministischem Fast Path und Reasoning.
//!
//! Das ArtifactNetwork bleibt vollständig unabhängig von LLMs. Erst diese
//! darüberliegende Runtime entscheidet, ob ein erfolgreicher Netzaufruf einen
//! bekannten Treffer geliefert hat oder ob kuratiertes Reasoning nötig ist.

use crate::reasoning::{ReasoningError, ReasoningRequest, ReasoningService, ValidatedProposal};
use std::collections::HashSet;

use crate::core::{SchemaId, SchemaRegistry, TrustLevel};
use crate::store::{
    ActivatedArtifact, ActiveSource, ArtifactNetwork, NetworkError, SqliteArtifactStore,
};

/// Core-seitige Eligibility-Regel für deterministische Netztreffer.
///
/// Eine gewichtete Kante allein macht ein beliebiges Artifact noch nicht zu
/// einer bekannten Route. Schema und Mindestvertrauen müssen explizit passen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRoutePolicy {
    pub eligible_schema_ids: Vec<SchemaId>,
    pub minimum_trust: TrustLevel,
}

impl KnownRoutePolicy {
    fn validate(&self, schemas: &SchemaRegistry) -> Result<(), KnownRoutePolicyError> {
        if self.eligible_schema_ids.is_empty() {
            return Err(KnownRoutePolicyError::EmptyEligibleSchemas);
        }

        let mut seen = HashSet::new();
        for schema_id in &self.eligible_schema_ids {
            if !seen.insert(schema_id) {
                return Err(KnownRoutePolicyError::DuplicateSchema(schema_id.clone()));
            }
            if schemas.get(schema_id).is_none() {
                return Err(KnownRoutePolicyError::UnregisteredSchema(schema_id.clone()));
            }
        }
        Ok(())
    }

    fn accepts(&self, candidate: &ActivatedArtifact) -> bool {
        self.eligible_schema_ids
            .contains(&candidate.artifact.schema_id)
            && trust_rank(candidate.artifact.trust.level) >= trust_rank(self.minimum_trust)
    }
}

/// Konfigurationsfehler einer deterministischen Route.
///
/// Eine ungültige Policy darf nie künstlich „keinen Treffer“ erzeugen und
/// dadurch einen kostenpflichtigen Reasoning-Fallback auslösen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownRoutePolicyError {
    EmptyEligibleSchemas,
    DuplicateSchema(SchemaId),
    UnregisteredSchema(SchemaId),
}

/// Ergebnis genau einer hybriden Routensuche.
#[derive(Debug)]
pub enum RouteResolution {
    /// Deterministisch aktivierte und durch die Route-Policy geprüfte Ziele.
    ///
    /// Auch diese Werte sind Kandidaten und noch keine Action-Autorisierung.
    KnownCandidates(Vec<ActivatedArtifact>),
    /// Validierte, aber weiterhin nicht autorisierte Reasoning-Vorschläge.
    ReasonedCandidates(Vec<ValidatedProposal>),
    /// Weder das bekannte Netz noch der Fallback lieferten einen Kandidaten.
    Unresolved,
}

/// Fehler werden sichtbar weitergegeben und niemals als „unbekannt“ umgedeutet.
#[derive(Debug)]
pub enum HybridRoutingError {
    Policy(KnownRoutePolicyError),
    Network(NetworkError),
    Reasoning(ReasoningError),
}

impl From<KnownRoutePolicyError> for HybridRoutingError {
    fn from(value: KnownRoutePolicyError) -> Self {
        Self::Policy(value)
    }
}

impl From<NetworkError> for HybridRoutingError {
    fn from(value: NetworkError) -> Self {
        Self::Network(value)
    }
}

impl From<ReasoningError> for HybridRoutingError {
    fn from(value: ReasoningError) -> Self {
        Self::Reasoning(value)
    }
}

pub struct HybridRouter<'store, 'service, 'resources> {
    store: &'store SqliteArtifactStore,
    reasoning: &'service mut ReasoningService<'resources>,
}

impl<'store, 'service, 'resources> HybridRouter<'store, 'service, 'resources> {
    pub fn new(
        store: &'store SqliteArtifactStore,
        reasoning: &'service mut ReasoningService<'resources>,
    ) -> Self {
        Self { store, reasoning }
    }

    /// Nutzt Reasoning ausschließlich nach einem erfolgreichen Fast-Path-Lauf
    /// ohne ausreichend aktiviertes Ziel.
    pub fn resolve(
        &mut self,
        sources: &[ActiveSource],
        threshold: f64,
        known_route_policy: &KnownRoutePolicy,
        reasoning_request: ReasoningRequest,
    ) -> Result<RouteResolution, HybridRoutingError> {
        // Fehlkonfiguration ist kein unbekannter Weltzustand. Die Policy wird
        // deshalb vor Netzlauf und insbesondere vor jedem Modellaufruf geprüft.
        known_route_policy.validate(self.reasoning.schemas())?;

        // Das Network wird nur für die Dauer des Fast Paths geborgt. Danach
        // kann der Gateway den auditierbaren ReasoningRequest speichern, ohne
        // dass gleichzeitig eine langfristige Store-Borrow im Router lebt.
        let mut known = ArtifactNetwork::new(self.store).propagate_many(sources, threshold)?;
        known.retain(|candidate| known_route_policy.accepts(candidate));
        if !known.is_empty() {
            return Ok(RouteResolution::KnownCandidates(known));
        }

        let proposals = self.reasoning.reason(reasoning_request)?;
        if proposals.is_empty() {
            Ok(RouteResolution::Unresolved)
        } else {
            Ok(RouteResolution::ReasonedCandidates(proposals))
        }
    }
}

fn trust_rank(level: TrustLevel) -> u8 {
    match level {
        TrustLevel::Low => 0,
        TrustLevel::Medium => 1,
        TrustLevel::High => 2,
    }
}

#[cfg(test)]
mod tests;

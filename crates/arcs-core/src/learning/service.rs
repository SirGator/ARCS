use crate::core::VersionId;
use crate::store::{ArtifactNetwork, NetworkError};

use super::{LearningError, LearningPolicy};

/// Explizite Gewichtsverstärkung bereits bestehender Artifact-Verbindungen.
pub struct LearningService<'a> {
    network: &'a ArtifactNetwork<'a>,
    policy: LearningPolicy,
}

impl<'a> LearningService<'a> {
    pub fn new(network: &'a ArtifactNetwork<'a>, policy: LearningPolicy) -> Self {
        Self { network, policy }
    }

    /// Verstärkt eine vorhandene Kante und liefert ihr neues Gewicht zurück.
    pub fn reinforce(&self, source: &VersionId, target: &VersionId) -> Result<f64, LearningError> {
        let edge = self
            .network
            .edge(source, target)?
            .ok_or_else(|| NetworkError::MissingEdge {
                from: source.clone(),
                to: target.clone(),
            })?;
        let increased = edge.weight + self.policy.success_increment;
        let new_weight = if increased > self.policy.max_weight {
            self.policy.max_weight
        } else {
            increased
        };
        self.network.set_weight(source, target, new_weight)?;
        Ok(new_weight)
    }
}

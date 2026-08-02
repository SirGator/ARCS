use crate::core::artifact::{ArtifactId, VersionId};

/// Vom Core erzeugtes Identitätspaar für eine neue unveränderliche Version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifactIds {
    pub artifact_id: ArtifactId,
    pub version_id: VersionId,
}

/// Injizierbare ID-Quelle, damit Adapter niemals ihre eigenen IDs bestimmen.
pub trait ArtifactIdGenerator: Send {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds;
}

/// Einfache monotone ID-Quelle für einen einzelnen Runtime-Knoten.
///
/// Dauerhafte Multi-Node-Installationen sollten denselben Port mit einer
/// persistenten beziehungsweise global eindeutigen Implementierung ersetzen.
pub struct SequenceIdGenerator {
    node_prefix: String,
    next_sequence: u64,
}

impl SequenceIdGenerator {
    pub fn new(node_prefix: impl Into<String>) -> Self {
        Self {
            node_prefix: node_prefix.into(),
            next_sequence: 1,
        }
    }
}

impl ArtifactIdGenerator for SequenceIdGenerator {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let artifact_id = format!("{}-{artifact_type}-{sequence}", self.node_prefix);
        GeneratedArtifactIds {
            artifact_id: ArtifactId(artifact_id.clone()),
            version_id: VersionId(format!("{artifact_id}-v1")),
        }
    }
}

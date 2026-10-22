//! Deterministische Host-Vorschläge ohne Modellaufruf oder Aktionsautorität.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::{AdapterRegistry, AdapterRegistryError, CapabilityRef};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryError, ArtifactFactoryInput,
    ArtifactIdGenerator, Clock, Provenance, SchemaId, SchemaRegistry, SchemaViolation, Source,
    SourceClass, SourceKind, SubjectId, Trust, TrustLevel, VersionId,
};
use crate::store::{Idempotency, SqliteArtifactStore, StoreError, relation_kinds};

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateProposal {
    pub proposal_id: String,
    pub rule_id: String,
    pub schema_id: SchemaId,
    pub payload: Value,
    pub evidence: Vec<VersionId>,
    pub required_capabilities: Vec<CapabilityRef>,
}

#[derive(Debug)]
pub enum ProposalError {
    InvalidProposal(&'static str),
    MissingSchema(SchemaId),
    NotCandidateSchema,
    MissingEvidence(VersionId),
    Authorization(AdapterRegistryError),
    InvalidPayload(Vec<SchemaViolation>),
    Store(StoreError),
    Factory(ArtifactFactoryError),
    Serialization(serde_json::Error),
}

/// Nur der vertrauenswürdige Host ruft diesen Dienst auf. Die Bedeutung einer
/// Regel bleibt außerhalb des Cores; Verification und Approval folgen separat.
pub struct ProposalService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> ProposalService<'a> {
    pub fn new(
        policy: &'a AdapterRegistry,
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            policy,
            schemas,
            store,
            ids,
            clock,
        }
    }

    pub fn propose(&mut self, proposal: CandidateProposal) -> Result<Artifact, ProposalError> {
        for text in [&proposal.proposal_id, &proposal.rule_id] {
            if text.trim().is_empty() || text.len() > 128 {
                return Err(ProposalError::InvalidProposal(
                    "proposal and rule IDs must contain 1..128 bytes",
                ));
            }
        }
        if proposal.evidence.is_empty() || proposal.required_capabilities.is_empty() {
            return Err(ProposalError::InvalidProposal(
                "evidence and capabilities are required",
            ));
        }
        let definition = self
            .schemas
            .get(&proposal.schema_id)
            .ok_or_else(|| ProposalError::MissingSchema(proposal.schema_id.clone()))?
            .clone();
        if definition.artifact_type != "candidate"
            && !definition.artifact_type.ends_with("_candidate")
        {
            return Err(ProposalError::NotCandidateSchema);
        }
        self.schemas
            .validate(&proposal.schema_id, &proposal.payload)
            .map_err(ProposalError::InvalidPayload)?;
        let mut seen = HashSet::new();
        for version in &proposal.evidence {
            if !seen.insert(version) {
                return Err(ProposalError::InvalidProposal("duplicate evidence"));
            }
            if self
                .store
                .get(version)
                .map_err(ProposalError::Store)?
                .is_none()
            {
                return Err(ProposalError::MissingEvidence(version.clone()));
            }
        }
        let mut capabilities = HashSet::new();
        for capability in &proposal.required_capabilities {
            if !capabilities.insert(capability) {
                return Err(ProposalError::InvalidProposal("duplicate capability"));
            }
            self.policy
                .authorized_capability(&capability.adapter_id, &capability.capability_id)
                .map_err(ProposalError::Authorization)?;
        }
        let input = serde_json::to_string(&proposal).map_err(ProposalError::Serialization)?;
        let key = serde_json::to_string(&("host_proposal", &proposal.proposal_id))
            .map_err(ProposalError::Serialization)?;
        let stream = format!("proposal:{}", proposal.proposal_id);
        let relations: Vec<_> = proposal
            .evidence
            .iter()
            .cloned()
            .map(|version| (version, relation_kinds::supported_by()))
            .collect();
        let artifact = ArtifactFactory::new(self.clock, self.ids)
            .create(ArtifactFactoryInput {
                schema: definition,
                created_by: Actor {
                    actor_type: ActorType::System,
                    id: "arcs.proposal".into(),
                },
                source: Source {
                    kind: SourceKind::Internal,
                    reference: stream.clone(),
                },
                trust: Trust {
                    level: TrustLevel::High,
                    source_class: SourceClass::System,
                },
                stream_key: stream.clone(),
                subject: SubjectId(stream),
                tags: proposal
                    .required_capabilities
                    .iter()
                    .map(|c| format!("requires:{}:{}", c.adapter_id.0, c.capability_id.0))
                    .collect(),
                payload: proposal.payload,
                provenance: Some(Provenance {
                    parents: proposal.evidence.iter().map(|v| v.0.clone()).collect(),
                    rules_applied: vec!["proposal.host_rule".into(), proposal.rule_id],
                    models_used: vec![],
                    transform: Some("proposal.deterministic".into()),
                }),
            })
            .map_err(ProposalError::Factory)?;
        self.store
            .append_current_related_once(
                &artifact,
                self.schemas,
                Idempotency {
                    key: &key,
                    input: &input,
                },
                &relations,
            )
            .map(|committed| committed.artifact)
            .map_err(ProposalError::Store)
    }
}

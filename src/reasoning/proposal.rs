//! Use Case zum einmaligen Persistieren eines validierten Modellkandidaten.

use super::service::ensure_candidate_schema;
use super::{ReasoningError, ReasoningService};
use crate::core::{
    Actor, ActorType, Artifact, ModelUse, Provenance, Source, SourceClass, SourceKind, Trust,
    TrustLevel,
};
use crate::reasoning::ValidatedProposal;
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_invocation_id,
};
use crate::store::relation_kinds;

impl ReasoningService<'_> {
    /// Speichert einen zuvor validierten Modellvorschlag als niedrig
    /// vertrauenswürdiges Candidate-Artifact. Dies autorisiert oder dispatcht
    /// die vorgeschlagenen Fähigkeiten ausdrücklich nicht.
    pub fn commit_proposal(
        &mut self,
        proposal: ValidatedProposal,
    ) -> Result<Artifact, ReasoningError> {
        for capability in &proposal.required_capabilities {
            if !self.policy.is_enabled_capability(capability) {
                return Err(ReasoningError::ForbiddenCandidateCapability(
                    capability.clone(),
                ));
            }
        }
        let candidate_index = proposal.candidate_index.to_string();
        let invocation_id = deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                &proposal.reasoning_capability.adapter_id.0,
                &proposal.reasoning_capability.capability_id.0,
                &proposal.reasoning_request_version.0,
                &candidate_index,
            ],
        );
        let dispatched = {
            let invocations = InvocationService::new(self.store, self.schemas, self.clock);
            let prepared = invocations.prepare(InvocationSpec {
                invocation_id,
                kind: InvocationKind::Reasoning,
                capability: format!(
                    "{}/{}",
                    proposal.reasoning_capability.adapter_id.0,
                    proposal.reasoning_capability.capability_id.0
                ),
                input_version: proposal.reasoning_request_version.clone(),
            })?;
            if prepared.status == InvocationStatus::Succeeded {
                let result = prepared
                    .result_version
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                return self
                    .store
                    .get(&result)?
                    .ok_or(crate::runtime::InvocationError::MissingResult)
                    .map_err(ReasoningError::from);
            }
            let recovered = invocations.recover(&prepared)?;
            invocations.dispatch(&recovered)?
        };
        ensure_candidate_schema(self.schemas, &proposal.schema_id)?;
        self.schemas
            .validate(&proposal.schema_id, &proposal.payload)
            .map_err(ReasoningError::InvalidPayload)?;
        let definition = self
            .schemas
            .get(&proposal.schema_id)
            .ok_or_else(|| ReasoningError::MissingRegisteredSchema(proposal.schema_id.clone()))?
            .clone();
        let generated = self.ids.next(&definition.artifact_type);
        let artifact = Artifact {
            artifact_id: generated.artifact_id,
            version_id: generated.version_id,
            version: 1,
            artifact_type: definition.artifact_type,
            schema_id: definition.id,
            schema_version: definition.version,
            created_at: self.clock.now_rfc3339(),
            created_by: Actor {
                actor_type: ActorType::Model,
                id: proposal.adapter_id.0.clone(),
            },
            source: Source {
                // Der Core erzeugt den sicheren Umschlag, der fachliche Inhalt
                // bleibt jedoch eine externe Modellausgabe.
                kind: SourceKind::External,
                reference: format!("reasoning:{}", proposal.request_id),
            },
            trust: Trust {
                level: TrustLevel::Low,
                source_class: SourceClass::Model,
            },
            stream_key: format!("reasoning:{}", proposal.request_id),
            subject: None,
            tags: proposal
                .required_capabilities
                .iter()
                .map(|capability| {
                    format!(
                        "requires:{}:{}",
                        capability.adapter_id.0, capability.capability_id.0
                    )
                })
                .chain([format!("adapter:{}", proposal.adapter_id.0)])
                .collect(),
            payload: proposal.payload,
            provenance: Some(Provenance {
                parents: proposal
                    .referenced_versions
                    .iter()
                    .map(|version| version.0.clone())
                    .collect(),
                rules_applied: vec![
                    "reasoning.context_minimized".into(),
                    "reasoning.proposal_schema_validated".into(),
                    "reasoning.candidate_capabilities_bounded".into(),
                ],
                models_used: vec![ModelUse {
                    name: proposal.trace.model_name,
                    prompt_hash: proposal.trace.prompt_hash,
                    inputs: proposal
                        .context_versions
                        .iter()
                        .map(|version| version.0.clone())
                        .collect(),
                    temperature: proposal.trace.temperature,
                    raw_output_hash: proposal.trace.raw_output_hash,
                }],
                transform: Some(format!("reasoning_adapter:{}", proposal.adapter_id.0)),
            }),
        };

        // Aktivierungsgewichte und semantische Nachvollziehbarkeit bleiben
        // getrennt: Diese Kanten dokumentieren Evidenz und Modellauftrag,
        // beeinflussen aber keine Network-Aktivierung.
        let relations = proposal
            .referenced_versions
            .iter()
            .cloned()
            .map(|version| (version, relation_kinds::supported_by()))
            .chain([(
                proposal.reasoning_request_version.clone(),
                relation_kinds::generated_by(),
            )])
            .collect::<Vec<_>>();
        InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
            &dispatched,
            &artifact,
            &relations,
        )?;
        Ok(artifact)
    }
}

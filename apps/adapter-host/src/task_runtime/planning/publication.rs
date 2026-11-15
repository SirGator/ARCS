//! Geprüfte Plan-Snapshots im gemeinsamen Artifact-Netz; keine Action-Approval.
use super::{ACCEPTED_PLAN, AcceptedPlan};
use crate::config::PLAN_ID;
use crate::task_runtime::{TaskRuntime, config::invalid};
use crate::{Host, HostError};
use arcs::adapters::{AdapterId, AdapterRegistry, CapabilityId};
use arcs::core::{ActorType, Artifact, SystemClock, VersionId};
use arcs::input::{InputMessage, InputService};
use arcs::store::{ArtifactRelations, relation_kinds};
use serde_json::json;

impl TaskRuntime {
    pub(super) fn publish_plan(
        &self,
        host: &Host,
        plan: &AcceptedPlan,
    ) -> Result<Artifact, HostError> {
        let payload = serde_json::to_value(plan).map_err(|e| invalid(e.to_string()))?;
        if serde_json::to_vec(&payload)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > 60_000
        {
            return Err(invalid("accepted plan snapshot exceeds 60000 bytes"));
        }
        let manifest=serde_json::from_value(json!({"protocol_version":1,"adapter_id":PLAN_ID,"adapter_version":"1","capabilities":[{"id":"plan.record","contract":{"kind":"input","emits":[ACCEPTED_PLAN]},"required_permissions":[]}]})).map_err(|e|invalid(e.to_string()))?;
        // A validated topology is still derived from an untrusted proposal.
        // It grants no authority and must not elevate model-derived truth.
        let grant=serde_json::from_value(json!({"adapter_id":PLAN_ID,"producer_class":"system","enabled_capabilities":["plan.record"],"granted_permissions":[],"assigned_trust":"low","ingress_source_kind":"internal","max_payload_bytes":65536,"max_external_reference_bytes":256,"reasoning_limits":null})).map_err(|e|invalid(e.to_string()))?;
        let mut registry = AdapterRegistry::new();
        registry
            .register(manifest, grant, host.schemas(), host.store())
            .map_err(HostError::Registration)?;
        let mut ids = host
            .store()
            .durable_id_generator("host-plan")
            .map_err(HostError::Store)?;
        let artifact = InputService::new(
            &registry,
            host.schemas(),
            host.store(),
            &mut ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(PLAN_ID.into()),
            InputMessage {
                capability_id: CapabilityId("plan.record".into()),
                external_subject: Some(plan.input_version_id.0.clone()),
                external_reference: arcs::runtime::deterministic_invocation_id(
                    arcs::runtime::InvocationKind::Request,
                    &[
                        "accepted-plan",
                        &self.config.runtime_id,
                        &plan.parent_sequence.to_string(),
                        &plan.proposal_version_id.0,
                    ],
                ),
                payload,
            },
        )
        .map_err(|source| HostError::Input {
            request: plan.parent_route_id.clone(),
            source,
        })?;
        let relations = ArtifactRelations::new(host.store());
        for (target, kind) in [
            (&plan.proposal_version_id, relation_kinds::based_on()),
            (&plan.input_version_id, relation_kinds::caused_by()),
            (
                &self.journal.definition_version()?,
                relation_kinds::based_on(),
            ),
        ] {
            if !relations
                .outgoing(&artifact.version_id)
                .map_err(HostError::Store)?
                .iter()
                .any(|r| &r.to == target && r.kind == kind)
            {
                relations
                    .connect(artifact.version_id.clone(), target.clone(), kind)
                    .map_err(HostError::Store)?;
            }
        }
        Ok(artifact)
    }

    pub(super) fn load_plan(
        &self,
        host: &Host,
        version: &VersionId,
    ) -> Result<AcceptedPlan, HostError> {
        let artifact = host
            .store()
            .get(version)
            .map_err(HostError::Store)?
            .ok_or_else(|| invalid("accepted plan artifact is missing"))?;
        if artifact.schema_id.0 != ACCEPTED_PLAN
            || artifact.created_by.id != PLAN_ID
            || artifact.created_by.actor_type != ActorType::System
        {
            return Err(invalid("untrusted accepted plan artifact"));
        }
        serde_json::from_value(artifact.payload)
            .map_err(|e| invalid(format!("invalid accepted plan snapshot: {e}")))
    }
}

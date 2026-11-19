//! Plan topology and replanning evidence live in the existing Artifact network.
use super::*;
use crate::config::PLAN_ID;
use crate::task_runtime::{TaskRuntime, config::invalid};
use crate::{Host, HostError};
use arcs::adapters::{AdapterId, AdapterRegistry, CapabilityId};
use arcs::core::{ActorType, Artifact, SystemClock, VersionId};
use arcs::input::{InputMessage, InputService};
use arcs::store::{ArtifactRelations, relation_kinds};
use serde_json::{Value, json};

impl TaskRuntime {
    fn record_plan_artifact(
        &self,
        host: &Host,
        capability: &str,
        input: &VersionId,
        reference: String,
        payload: Value,
    ) -> Result<Artifact, HostError> {
        if serde_json::to_vec(&payload)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > 60_000
        {
            return Err(invalid("plan snapshot exceeds 60000 bytes"));
        }
        let caps = [
            ("plan.record", ACCEPTED_PLAN),
            ("plan.record_versioned", VERSIONED_PLAN),
            ("plan.replanning", REPLANNING_CONTEXT),
        ];
        let manifest=serde_json::from_value(json!({"protocol_version":1,"adapter_id":PLAN_ID,"adapter_version":"1","capabilities":caps.iter().map(|(id,schema)|json!({"id":id,"contract":{"kind":"input","emits":[schema]},"required_permissions":[]})).collect::<Vec<_>>()})).map_err(|e|invalid(e.to_string()))?;
        // Recording checked topology does not turn model-derived content into truth.
        let grant=serde_json::from_value(json!({"adapter_id":PLAN_ID,"producer_class":"system","enabled_capabilities":caps.iter().map(|(id,_)|*id).collect::<Vec<_>>(),"granted_permissions":[],"assigned_trust":"low","ingress_source_kind":"internal","max_payload_bytes":65536,"max_external_reference_bytes":256,"reasoning_limits":null})).map_err(|e|invalid(e.to_string()))?;
        let mut registry = AdapterRegistry::new();
        registry
            .register(manifest, grant, host.schemas(), host.store())
            .map_err(HostError::Registration)?;
        let mut ids = host
            .store()
            .durable_id_generator("host-plan")
            .map_err(HostError::Store)?;
        InputService::new(
            &registry,
            host.schemas(),
            host.store(),
            &mut ids,
            &SystemClock,
        )
        .ingest(
            &AdapterId(PLAN_ID.into()),
            InputMessage {
                capability_id: CapabilityId(capability.into()),
                external_subject: Some(input.0.clone()),
                external_reference: reference,
                payload,
            },
        )
        .map_err(|source| HostError::Input {
            request: "plan publication".into(),
            source,
        })
    }
    fn relate_plan(
        &self,
        host: &Host,
        artifact: &Artifact,
        parents: &[(VersionId, arcs::store::RelationKind)],
    ) -> Result<(), HostError> {
        let relations = ArtifactRelations::new(host.store());
        for (target, kind) in parents {
            if !relations
                .outgoing(&artifact.version_id)
                .map_err(HostError::Store)?
                .iter()
                .any(|r| &r.to == target && &r.kind == kind)
            {
                relations
                    .connect(artifact.version_id.clone(), target.clone(), kind.clone())
                    .map_err(HostError::Store)?;
            }
        }
        Ok(())
    }
    pub(super) fn publish_plan(
        &self,
        host: &Host,
        plan: &AcceptedPlan,
    ) -> Result<Artifact, HostError> {
        let reference = arcs::runtime::deterministic_invocation_id(
            arcs::runtime::InvocationKind::Request,
            &[
                "accepted-plan",
                &self.config.runtime_id,
                &plan.parent_sequence.to_string(),
                &plan.proposal_version_id.0,
            ],
        );
        let artifact = self.record_plan_artifact(
            host,
            if plan.revision.is_some() {
                "plan.record_versioned"
            } else {
                "plan.record"
            },
            &plan.input_version_id,
            reference,
            serde_json::to_value(plan).map_err(|e| invalid(e.to_string()))?,
        )?;
        let mut parents = vec![
            (plan.proposal_version_id.clone(), relation_kinds::based_on()),
            (plan.input_version_id.clone(), relation_kinds::caused_by()),
            (
                self.journal.definition_version()?,
                relation_kinds::based_on(),
            ),
        ];
        if let Some(revision) = &plan.revision {
            if let Some(previous) = &revision.previous_plan_version_id {
                parents.push((previous.clone(), relation_kinds::supersedes()));
            }
            if let Some(context) = &revision.context_version_id {
                parents.push((context.clone(), relation_kinds::based_on()));
            }
        }
        self.relate_plan(host, &artifact, &parents)?;
        Ok(artifact)
    }
    pub(super) fn publish_replanning_context(
        &self,
        host: &Host,
        context: &ReplanningContext,
    ) -> Result<Artifact, HostError> {
        let reference = arcs::runtime::deterministic_invocation_id(
            arcs::runtime::InvocationKind::Request,
            &[
                "plan-replanning",
                &self.config.runtime_id,
                &context.parent_sequence.to_string(),
                &context.revision.to_string(),
                &context.previous_plan_version_id.0,
            ],
        );
        let artifact = self.record_plan_artifact(
            host,
            "plan.replanning",
            &context.input_version_id,
            reference,
            serde_json::to_value(context).map_err(|e| invalid(e.to_string()))?,
        )?;
        let mut parents = vec![
            (
                context.previous_plan_version_id.clone(),
                relation_kinds::based_on(),
            ),
            (
                context.previous_proposal_version_id.clone(),
                relation_kinds::based_on(),
            ),
            (
                context.input_version_id.clone(),
                relation_kinds::caused_by(),
            ),
        ];
        parents.extend(
            context
                .context
                .iter()
                .map(|c| (c.version_id.clone(), relation_kinds::based_on())),
        );
        self.relate_plan(host, &artifact, &parents)?;
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
        if ![ACCEPTED_PLAN, VERSIONED_PLAN].contains(&artifact.schema_id.0.as_str())
            || artifact.created_by.id != PLAN_ID
            || artifact.created_by.actor_type != ActorType::System
        {
            return Err(invalid("untrusted accepted plan artifact"));
        }
        let versioned = artifact.schema_id.0 == VERSIONED_PLAN;
        let plan: AcceptedPlan = serde_json::from_value(artifact.payload)
            .map_err(|e| invalid(format!("invalid accepted plan snapshot: {e}")))?;
        if versioned != plan.revision.is_some() {
            return Err(invalid("plan schema and revision disagree"));
        }
        Ok(plan)
    }
}

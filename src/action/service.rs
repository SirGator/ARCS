use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::{AdapterRegistry, CapabilityContract, CapabilityRef};
use crate::approval::ApprovalDecision;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::ActionError;

const ACTION_SCHEMA_ID: &str = "arcs.action.v1";
const APPROVAL_SCHEMA_ID: &str = "arcs.approval.v1";

#[derive(Deserialize)]
struct ApprovalGatePayload {
    target_version: String,
    decision: ApprovalDecision,
}

#[derive(Serialize)]
struct ActionPayload {
    target_version: String,
    approval_version: String,
    capability: CapabilityRef,
    input_schema_id: SchemaId,
    payload: Value,
}

/// Materialisiert Autorität zu einem kontrollierten Auftrag, ohne ihn auszuführen.
pub struct ActionService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> ActionService<'a> {
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

    pub fn materialize(
        &mut self,
        approval_version: &VersionId,
        capability: &CapabilityRef,
    ) -> Result<Artifact, ActionError> {
        let approval = self
            .store
            .get(approval_version)?
            .ok_or_else(|| ActionError::MissingApproval(approval_version.clone()))?;
        if approval.schema_id != SchemaId(APPROVAL_SCHEMA_ID.into()) {
            return Err(ActionError::NotApprovalArtifact);
        }
        let gate: ApprovalGatePayload = serde_json::from_value(approval.payload)?;
        if gate.decision != ApprovalDecision::Approved {
            return Err(ActionError::ApprovalNotApproved);
        }

        let target = VersionId(gate.target_version);
        let approves_target = self
            .store
            .outgoing_relations(approval_version)?
            .into_iter()
            .any(|relation| relation.to == target && relation.kind == relation_kinds::approves());
        if !approves_target {
            return Err(ActionError::MissingApprovalRelation);
        }
        let candidate = self
            .store
            .get(&target)?
            .ok_or_else(|| ActionError::MissingCandidate(target.clone()))?;
        if candidate.artifact_type != "candidate"
            && !candidate.artifact_type.ends_with("_candidate")
        {
            return Err(ActionError::ApprovalTargetIsNotCandidate);
        }

        let (_, descriptor) = self
            .policy
            .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
        let CapabilityContract::Act {
            accepts,
            idempotent,
            ..
        } = &descriptor.contract
        else {
            return Err(ActionError::CapabilityIsNotAct(capability.clone()));
        };
        if !idempotent {
            return Err(ActionError::ActCapabilityIsNotIdempotent(
                capability.clone(),
            ));
        }
        if !accepts.contains(&candidate.schema_id) {
            return Err(ActionError::CandidateSchemaNotAccepted {
                capability: capability.clone(),
                schema: candidate.schema_id.clone(),
            });
        }
        let required_tag = format!(
            "requires:{}:{}",
            capability.adapter_id.0, capability.capability_id.0
        );
        if !candidate.tags.iter().any(|tag| tag == &required_tag) {
            return Err(ActionError::CapabilityNotRequired(capability.clone()));
        }

        let schema_id = SchemaId(ACTION_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| ActionError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let payload = serde_json::to_value(ActionPayload {
            target_version: candidate.version_id.0.clone(),
            approval_version: approval_version.0.clone(),
            capability: capability.clone(),
            input_schema_id: candidate.schema_id.clone(),
            payload: candidate.payload.clone(),
        })?;
        let subject = SubjectId(format!(
            "action:{}:{}:{}",
            approval_version.0, capability.adapter_id.0, capability.capability_id.0
        ));
        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let action = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.action".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("action:{}", approval_version.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: subject.0.clone(),
            subject,
            tags: vec![
                format!("target:{}", candidate.version_id.0),
                format!("approval:{}", approval_version.0),
                format!(
                    "capability:{}:{}",
                    capability.adapter_id.0, capability.capability_id.0
                ),
            ],
            payload,
            provenance: Some(Provenance {
                parents: vec![candidate.version_id.0.clone(), approval_version.0.clone()],
                rules_applied: vec![
                    "action.approval_verified".into(),
                    "action.capability_authorized".into(),
                    "action.materialized".into(),
                ],
                models_used: vec![],
                transform: Some("action.materialization".into()),
            }),
        })?;
        self.store.append_related(
            &action,
            self.schemas,
            &[
                (candidate.version_id, relation_kinds::derived_from()),
                (approval_version.clone(), relation_kinds::based_on()),
            ],
        )?;
        Ok(action)
    }
}

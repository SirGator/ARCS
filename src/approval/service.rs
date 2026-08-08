use serde::{Deserialize, Serialize};

use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};
use crate::verification::VerificationVerdict;

use super::{ApprovalDecision, ApprovalError};

const APPROVAL_SCHEMA_ID: &str = "arcs.approval.v1";
const VERIFICATION_REPORT_SCHEMA_ID: &str = "arcs.verification_report.v1";

#[derive(Debug, Deserialize)]
struct VerificationGatePayload {
    target_version: String,
    verdict: VerificationVerdict,
}

#[derive(Serialize)]
struct ApprovalPayload {
    target_version: String,
    verification_report_version: String,
    decision: ApprovalDecision,
    reason: String,
}

/// Erzeugt auditierbare Autoritätsentscheidungen, ohne eine Aktion auszuführen.
pub struct ApprovalService<'a> {
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> ApprovalService<'a> {
    pub fn new(
        schemas: &'a SchemaRegistry,
        store: &'a SqliteArtifactStore,
        ids: &'a mut dyn ArtifactIdGenerator,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            schemas,
            store,
            ids,
            clock,
        }
    }

    pub fn decide(
        &mut self,
        target: &VersionId,
        verification_report: &VersionId,
        decision: ApprovalDecision,
        approver: Actor,
        reason: String,
    ) -> Result<Artifact, ApprovalError> {
        self.store
            .get(target)?
            .ok_or_else(|| ApprovalError::MissingTarget(target.clone()))?;
        let report = self
            .store
            .get(verification_report)?
            .ok_or_else(|| ApprovalError::MissingVerificationReport(verification_report.clone()))?;

        if report.schema_id != SchemaId(VERIFICATION_REPORT_SCHEMA_ID.into()) {
            return Err(ApprovalError::NotVerificationReport);
        }

        let verification: VerificationGatePayload = serde_json::from_value(report.payload)?;
        if verification.target_version != target.0 {
            return Err(ApprovalError::VerificationTargetMismatch);
        }
        let verifies_target = self
            .store
            .outgoing_relations(verification_report)?
            .into_iter()
            .any(|relation| relation.to == *target && relation.kind == relation_kinds::verifies());
        if !verifies_target {
            return Err(ApprovalError::MissingVerificationRelation);
        }

        match (verification.verdict, decision) {
            (VerificationVerdict::Pass, ApprovalDecision::Approved) => {}
            (VerificationVerdict::Fail, ApprovalDecision::Approved) => {
                return Err(ApprovalError::CannotApproveFailedVerification);
            }
            (VerificationVerdict::Unknown, ApprovalDecision::Approved) => {
                return Err(ApprovalError::CannotApproveUnknownVerification);
            }
            (_, ApprovalDecision::Rejected) => {}
        }

        let source_class = match approver.actor_type {
            ActorType::Human => SourceClass::Human,
            ActorType::System => SourceClass::System,
            ActorType::Adapter | ActorType::Model | ActorType::Executor => {
                return Err(ApprovalError::UnauthorizedApprover);
            }
        };

        let schema_id = SchemaId(APPROVAL_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| ApprovalError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let payload = serde_json::to_value(ApprovalPayload {
            target_version: target.0.clone(),
            verification_report_version: verification_report.0.clone(),
            decision,
            reason,
        })?;
        let approval_subject = SubjectId(format!("approval:{}", target.0));
        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let approval = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: approver,
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("approval:{}", target.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class,
            },
            stream_key: format!("approval:{}", target.0),
            subject: approval_subject,
            tags: vec![
                format!("target:{}", target.0),
                format!("verification:{}", verification_report.0),
            ],
            payload,
            provenance: Some(Provenance {
                parents: vec![target.0.clone(), verification_report.0.clone()],
                rules_applied: vec!["approval.decision".into()],
                models_used: vec![],
                transform: Some("approval.gate".into()),
            }),
        })?;

        let target_relation = match decision {
            ApprovalDecision::Approved => relation_kinds::approves(),
            ApprovalDecision::Rejected => relation_kinds::rejects(),
        };
        self.store.append_related(
            &approval,
            self.schemas,
            &[
                (target.clone(), target_relation),
                (verification_report.clone(), relation_kinds::based_on()),
            ],
        )?;

        Ok(approval)
    }
}

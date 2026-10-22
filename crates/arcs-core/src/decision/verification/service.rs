use serde::Serialize;

use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust,
    TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::{VerificationError, VerificationFinding, VerificationVerdict, Verifier};

const VERIFICATION_REPORT_SCHEMA_ID: &str = "arcs.verification_report.v1";

#[derive(Serialize)]
struct VerificationReportPayload {
    target_version: String,
    verdict: VerificationVerdict,
    findings: Vec<VerificationFinding>,
}

/// Persistiert Aussagen eines Verifiers ohne Approval- oder Execution-Wirkung.
pub struct VerificationService<'a> {
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> VerificationService<'a> {
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

    pub fn verify(
        &mut self,
        target: &VersionId,
        verifier: &dyn Verifier,
    ) -> Result<Artifact, VerificationError> {
        let target_artifact = self
            .store
            .get(target)?
            .ok_or_else(|| VerificationError::MissingTarget(target.clone()))?;
        let result = verifier.verify(&target_artifact)?;
        let schema_id = SchemaId(VERIFICATION_REPORT_SCHEMA_ID.into());
        let definition = self
            .schemas
            .get(&schema_id)
            .ok_or_else(|| VerificationError::MissingRegisteredSchema(schema_id.clone()))?
            .clone();
        let payload = serde_json::to_value(VerificationReportPayload {
            target_version: target.0.clone(),
            verdict: result.verdict,
            findings: result.findings,
        })?;
        let report_subject = SubjectId(format!("verification:{}", target.0));
        let mut factory = ArtifactFactory::new(self.clock, self.ids);
        let report = factory.create(ArtifactFactoryInput {
            schema: definition,
            created_by: Actor {
                actor_type: ActorType::System,
                id: "arcs.verification".into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: format!("verification:{}", target.0),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: format!("verification:{}", target.0),
            subject: report_subject,
            tags: vec![format!("target:{}", target.0)],
            payload,
            provenance: Some(Provenance {
                parents: vec![target.0.clone()],
                rules_applied: vec!["verification.executed".into()],
                models_used: vec![],
                transform: Some("verification.report".into()),
            }),
        })?;

        self.store.append_related(
            &report,
            self.schemas,
            &[(target.clone(), relation_kinds::verifies())],
        )?;
        Ok(report)
    }
}

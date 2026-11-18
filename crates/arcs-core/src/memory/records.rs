use super::*;
use crate::runtime::{InvocationKind, deterministic_invocation_id};

fn subject(target: &VersionId) -> SubjectId {
    // Source versions may already occupy the full 512-byte envelope limit.
    // The record below verifies the complete target, including hash collisions.
    SubjectId(format!("memory-assessment:{}", identity(target)))
}
fn identity(target: &VersionId) -> String {
    deterministic_invocation_id(InvocationKind::Request, &["memory-assessment", &target.0])
}

pub(super) fn validate_references(
    service: &MemoryAssessmentService<'_>,
    command: &MemoryAssessmentCommand,
) -> Result<(), MemoryError> {
    let fields = [crate::store::database::MetadataField::value(
        "subject",
        "$.subject",
    )];
    let target = service
        .store
        .metadata_projection(&command.target_version_id, &fields, 4096)?
        .ok_or_else(|| invalid("missing memory assessment target"))?;
    for id in command
        .evidence
        .iter()
        .chain(command.replacement_version_id.iter())
    {
        let evidence = service
            .store
            .metadata_projection(
                id,
                &[crate::store::database::MetadataField::value(
                    "subject",
                    "$.subject",
                )],
                4096,
            )?
            .ok_or_else(|| invalid("missing memory assessment evidence"))?;
        if command.replacement_version_id.as_ref() == Some(id)
            && evidence.schema_id != target.schema_id
        {
            return Err(invalid("replacement must use the target schema"));
        }
    }
    Ok(())
}

pub(super) fn load_at(
    service: &MemoryAssessmentService<'_>,
    target: &VersionId,
    through: u64,
) -> Result<Option<(Artifact, MemoryAssessment)>, MemoryError> {
    let Some(row) = service.store.schema_subject_head_at(
        &SchemaId(SCHEMA.into()),
        &subject(target),
        through,
        65536,
    )?
    else {
        return Ok(None);
    };
    let artifact = row
        .artifact
        .ok_or_else(|| invalid("oversized memory assessment"))?;
    validate_artifact(&artifact, service.schemas)
        .map_err(|_| invalid("invalid memory assessment artifact"))?;
    let command: MemoryAssessmentCommand = serde_json::from_value(artifact.payload.clone())
        .map_err(|_| invalid("invalid memory assessment payload"))?;
    command.validate()?;
    let id = identity(target);
    let mut parents = command
        .evidence
        .iter()
        .map(|id| id.0.clone())
        .collect::<std::collections::BTreeSet<_>>();
    parents.insert(target.0.clone());
    parents.extend(command.expected_version_id.iter().map(|id| id.0.clone()));
    parents.extend(command.replacement_version_id.iter().map(|id| id.0.clone()));
    if artifact.created_by
        != (Actor {
            actor_type: ActorType::System,
            id: PUBLISHER.into(),
        })
        || artifact.source.kind != SourceKind::Internal
        || artifact.trust
            != (Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            })
        || command.target_version_id != *target
        || artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&subject(target))
        || artifact.stream_key != subject(target).0
        || artifact.source.reference != artifact.stream_key
        || command.expected_version_id
            != (artifact.version > 1).then(|| VersionId(format!("{id}:v{}", artifact.version - 1)))
        || artifact.provenance.as_ref().is_none_or(|p| {
            p.parents
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                != parents
        })
    {
        return Err(invalid("untrusted memory assessment or changed identity"));
    }
    let receipt = MemoryAssessment {
        version_id: artifact.version_id.clone(),
        command,
    };
    Ok(Some((artifact, receipt)))
}

pub(super) fn commit(
    service: &MemoryAssessmentService<'_>,
    command: &MemoryAssessmentCommand,
    previous: Option<&Artifact>,
) -> Result<MemoryAssessment, MemoryError> {
    let id = identity(&command.target_version_id);
    let version = previous
        .map_or(Some(1), |a| a.version.checked_add(1))
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| invalid("memory assessment revision exhausted"))?;
    let schema = service
        .schemas
        .get(&SchemaId(SCHEMA.into()))
        .ok_or_else(|| invalid("missing memory assessment schema"))?;
    let mut parents = command
        .evidence
        .iter()
        .map(|id| id.0.clone())
        .collect::<std::collections::BTreeSet<_>>();
    parents.insert(command.target_version_id.0.clone());
    parents.extend(command.expected_version_id.iter().map(|id| id.0.clone()));
    parents.extend(command.replacement_version_id.iter().map(|id| id.0.clone()));
    let artifact = Artifact {
        artifact_id: ArtifactId(id.clone()),
        version_id: VersionId(format!("{id}:v{version}")),
        version,
        artifact_type: schema.artifact_type.clone(),
        schema_id: schema.id.clone(),
        schema_version: schema.version,
        created_at: service.clock.now_rfc3339(),
        created_by: Actor {
            actor_type: ActorType::System,
            id: PUBLISHER.into(),
        },
        source: Source {
            kind: SourceKind::Internal,
            reference: subject(&command.target_version_id).0.clone(),
        },
        trust: Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        stream_key: subject(&command.target_version_id).0.clone(),
        subject: Some(subject(&command.target_version_id)),
        tags: vec![],
        payload: serde_json::to_value(command)
            .map_err(|_| invalid("invalid memory assessment payload"))?,
        provenance: Some(Provenance {
            parents: parents.into_iter().collect(),
            rules_applied: vec!["memory.operator_assessment.v1".into()],
            ..Default::default()
        }),
    };
    match service.store.append_current_compared(
        std::slice::from_ref(&artifact),
        service.schemas,
        &[(
            subject(&command.target_version_id),
            SchemaId(SCHEMA.into()),
            previous.map(|a| a.version_id.clone()),
        )],
    ) {
        Ok(()) => Ok(MemoryAssessment {
            version_id: artifact.version_id,
            command: command.clone(),
        }),
        Err(StoreError::IdempotencyConflict | StoreError::VersionConflict { .. }) => {
            // Concurrent identical commands converge on the winner's receipt.
            if let Some(receipt) = service.current(&command.target_version_id)?
                && receipt.command == *command
            {
                return Ok(receipt);
            }
            Err(MemoryError::Conflict)
        }
        Err(error) => Err(error.into()),
    }
}

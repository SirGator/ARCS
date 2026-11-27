use super::*;
use crate::core::security::SecurityProfile;
use crate::runtime::{InvocationKind, deterministic_invocation_id};
use crate::store::database::Idempotency;

pub(super) fn validate_definition(
    registry: &AdapterRegistry,
    definition: &ReconnectDefinition,
) -> Result<(), ReconnectError> {
    definition.policy.validate()?;
    let installed = registry
        .get(&definition.manifest.adapter_id)
        .ok_or_else(|| invalid("reconnect installation is not registered"))?;
    if definition.capabilities_verified
        || definition.node_id.is_empty()
        || definition.node_id.len() > 64
        || !definition
            .node_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || installed.manifest() != &definition.manifest
        || installed.grant() != &definition.grant
        || definition.manifest.capabilities.is_empty()
        || definition
            .manifest
            .capabilities
            .iter()
            .any(|cap| !cap.contract.is_reasoning())
        || definition.grant.adapter_id != definition.manifest.adapter_id
        || serde_json::to_vec(definition)
            .map_err(|_| invalid("invalid reconnect definition"))?
            .len()
            > 60_000
    {
        return Err(invalid(
            "automatic reconnect requires a registered pure Reason installation and bounded definition",
        ));
    }
    Ok(())
}
fn definition_key(definition: &ReconnectDefinition) -> Result<String, ReconnectError> {
    Ok(format!(
        "adapter-reconnect:{}",
        deterministic_invocation_id(
            InvocationKind::Reasoning,
            &[
                &definition.node_id,
                &serde_json::to_string(definition)
                    .map_err(|_| invalid("invalid reconnect definition"))?
            ]
        )
    ))
}
fn state_identity(definition: &VersionId) -> String {
    deterministic_invocation_id(
        InvocationKind::Reasoning,
        &["adapter-reconnect-state", &definition.0],
    )
}
pub(super) fn subject(definition: &VersionId) -> SubjectId {
    SubjectId(format!("adapter-reconnect-state:{}", definition.0))
}
fn trusted(
    artifact: &Artifact,
    schema: &str,
    schemas: &SchemaRegistry,
) -> Result<(), ReconnectError> {
    validate_artifact(artifact, schemas).map_err(|_| invalid("invalid reconnect artifact"))?;
    if artifact.schema_id.0 != schema
        || artifact.created_by.actor_type != ActorType::System
        || artifact.created_by.id != PUBLISHER
        || artifact.source.kind != SourceKind::Internal
        || artifact.trust.level != TrustLevel::High
        || artifact.trust.source_class != SourceClass::System
    {
        return Err(invalid("untrusted reconnect artifact"));
    }
    Ok(())
}
pub(super) fn persist_definition(
    service: &mut ReconnectService<'_>,
    definition: &ReconnectDefinition,
) -> Result<Artifact, ReconnectError> {
    let key = definition_key(definition)?;
    let payload =
        serde_json::to_value(definition).map_err(|_| invalid("invalid reconnect definition"))?;
    let identity = payload.to_string();
    let artifact = ArtifactFactory::new(service.clock, service.ids)
        .create(ArtifactFactoryInput {
            schema: service
                .schemas
                .get(&SchemaId(SCHEMA.into()))
                .ok_or_else(|| invalid("missing reconnect schema"))?
                .clone(),
            created_by: Actor {
                actor_type: ActorType::System,
                id: PUBLISHER.into(),
            },
            source: Source {
                kind: SourceKind::Internal,
                reference: key.clone(),
            },
            trust: Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            stream_key: key.clone(),
            subject: SubjectId(key.clone()),
            tags: vec![],
            payload,
            provenance: Some(Provenance {
                rules_applied: vec!["adapter.reconnect.definition.v1".into()],
                ..Default::default()
            }),
        })
        .map_err(|_| invalid("cannot construct reconnect definition"))?;
    let saved = service
        .store
        .append_current_related_once(
            &artifact,
            service.schemas,
            Idempotency {
                key: &key,
                input: &identity,
            },
            &[],
        )?
        .artifact;
    let loaded = load_definition(service, &saved.version_id)?;
    if loaded != *definition {
        return Err(invalid("reconnect definition identity changed"));
    }
    Ok(saved)
}
pub(super) fn load_definition(
    service: &ReconnectService<'_>,
    version: &VersionId,
) -> Result<ReconnectDefinition, ReconnectError> {
    let artifact = service
        .store
        .get(version)?
        .ok_or_else(|| invalid("missing reconnect definition"))?;
    trusted(&artifact, SCHEMA, service.schemas)?;
    let definition: ReconnectDefinition = serde_json::from_value(artifact.payload.clone())
        .map_err(|_| invalid("invalid reconnect definition payload"))?;
    validate_definition(service.registry, &definition)?;
    let key = definition_key(&definition)?;
    if artifact.version != 1
        || artifact.stream_key != key
        || artifact.subject.as_ref() != Some(&SubjectId(key.clone()))
        || artifact.source.reference != key
        || service
            .store
            .find_by_stream_key(&key)?
            .is_none_or(|saved| saved.version_id != *version)
    {
        return Err(invalid("reconnect definition identity changed"));
    }
    Ok(definition)
}
pub(super) fn load_state(
    service: &ReconnectService<'_>,
    definition: &VersionId,
) -> Result<Option<(Artifact, ReconnectReceipt)>, ReconnectError> {
    let spec = load_definition(service, definition)?;
    let Some(artifact) = service
        .store
        .current(&subject(definition), &SchemaId(STATE_SCHEMA.into()))?
    else {
        return Ok(None);
    };
    trusted(&artifact, STATE_SCHEMA, service.schemas)?;
    let record: ReconnectState = serde_json::from_value(artifact.payload.clone())
        .map_err(|_| invalid("invalid reconnect state payload"))?;
    let id = state_identity(definition);
    if artifact.artifact_id.0 != id
        || artifact.version_id.0 != format!("{id}:v{}", artifact.version)
        || artifact.subject.as_ref() != Some(&subject(definition))
        || artifact.stream_key != subject(definition).0
        || artifact.source.reference != artifact.stream_key
        || (artifact.version > 1
            && artifact.provenance.as_ref().is_none_or(|p| {
                !p.parents
                    .contains(&format!("{id}:v{}", artifact.version - 1))
            }))
        || record.definition_version_id != *definition
        || record.generation > i64::MAX as u64
        || record.attempt > spec.policy.max_attempts
        || artifact
            .provenance
            .as_ref()
            .is_none_or(|p| !p.parents.contains(&definition.0))
        || record
            .not_before_unix_ms
            .is_some_and(|n| n > i64::MAX as u64)
    {
        return Err(invalid("reconnect state identity changed"));
    }
    let valid = match record.state {
        ConnectionState::Healthy => record.not_before_unix_ms.is_none() && record.failure.is_none(),
        ConnectionState::Waiting => {
            record.attempt < spec.policy.max_attempts && record.not_before_unix_ms.is_some()
        }
        ConnectionState::Connecting => {
            record.attempt > 0 && record.not_before_unix_ms.is_none() && record.failure.is_none()
        }
        ConnectionState::Exhausted => {
            record.attempt == spec.policy.max_attempts
                && record.not_before_unix_ms.is_none()
                && record.failure.is_some()
        }
        ConnectionState::Rejected => {
            record.attempt > 0
                && record.not_before_unix_ms.is_none()
                && record.failure == Some(ConnectionFailure::Contract)
        }
    };
    if !valid {
        return Err(invalid("invalid reconnect state transition data"));
    }
    let receipt = ReconnectReceipt {
        version_id: artifact.version_id.clone(),
        record,
    };
    Ok(Some((artifact, receipt)))
}
pub(super) fn update(
    service: &ReconnectService<'_>,
    record: &ReconnectState,
    previous: Option<&Artifact>,
) -> Result<ReconnectReceipt, ReconnectError> {
    let id = state_identity(&record.definition_version_id);
    let version = previous
        .map_or(Some(1), |a| a.version.checked_add(1))
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| invalid("reconnect revision overflow"))?;
    let schema = service
        .schemas
        .get(&SchemaId(STATE_SCHEMA.into()))
        .ok_or_else(|| invalid("missing reconnect state schema"))?;
    let mut parents = vec![record.definition_version_id.0.clone()];
    if let Some(previous) = previous {
        parents.push(previous.version_id.0.clone());
    }
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
            reference: subject(&record.definition_version_id).0.clone(),
        },
        trust: Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        stream_key: subject(&record.definition_version_id).0.clone(),
        subject: Some(subject(&record.definition_version_id)),
        tags: vec![],
        payload: serde_json::to_value(record).map_err(|_| invalid("invalid reconnect state"))?,
        provenance: Some(Provenance {
            parents,
            rules_applied: vec!["adapter.reconnect.state.v1".into()],
            ..Default::default()
        }),
        security: SecurityProfile::default(),
    };
    match service.store.append_current_compared(
        std::slice::from_ref(&artifact),
        service.schemas,
        &[(
            subject(&record.definition_version_id),
            SchemaId(STATE_SCHEMA.into()),
            previous.map(|a| a.version_id.clone()),
        )],
    ) {
        Ok(()) => Ok(ReconnectReceipt {
            version_id: artifact.version_id,
            record: record.clone(),
        }),
        Err(StoreError::IdempotencyConflict | StoreError::VersionConflict { .. }) => {
            Err(ReconnectError::Conflict)
        }
        Err(e) => Err(e.into()),
    }
}

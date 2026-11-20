use serde::Deserialize;
use serde_json::Value;

use crate::adapters::{
    ActAdapter, ActFence, ActInvocation, ActResponse, AdapterCallError, AdapterGrant,
    AdapterRegistry, CapabilityRef, ProducerClass,
};
use crate::approval::ApprovalDecision;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactFactory, ArtifactFactoryInput, ArtifactIdGenerator, Clock,
    Provenance, SchemaDefinition, SchemaId, SchemaRegistry, Source, SourceClass, SourceKind,
    SubjectId, Trust, VersionId,
};
use crate::runtime::reservations::{Reservation, ReservationService};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationState, InvocationStatus,
    deterministic_input_fingerprint, deterministic_invocation_id,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

use super::ExecutionError;

const ACTION_SCHEMA_ID: &str = "arcs.action.v1";
const APPROVAL_SCHEMA_ID: &str = "arcs.approval.v1";

#[derive(Deserialize)]
struct MaterializedActionPayload {
    target_version: String,
    approval_version: String,
    capability: CapabilityRef,
    input_schema_id: SchemaId,
    payload: Value,
}

#[derive(Deserialize)]
struct ApprovalGatePayload {
    target_version: String,
    verification_report_version: String,
    decision: ApprovalDecision,
}

pub enum ExecutionPreparation {
    Complete(Box<Artifact>),
    Dispatch(Box<PreparedExecution>),
}

/// Core-owned context for validating and recording a dispatched action's response.
pub struct PreparedExecution {
    call: ExecutionCall,
    dispatched: InvocationState,
    grant: AdapterGrant,
    result_definition: SchemaDefinition,
    approval_version: VersionId,
    candidate_version: VersionId,
}

impl PreparedExecution {
    pub fn call(&self) -> &ExecutionCall {
        &self.call
    }
}

/// An authorized dispatch containing only the data needed at the transport boundary.
#[derive(Clone)]
pub struct ExecutionCall {
    invocation: ActInvocation,
    fence: Option<ActFence>,
}

impl ExecutionCall {
    pub fn invocation(&self) -> &ActInvocation {
        &self.invocation
    }

    pub fn send(&self, endpoint: &dyn ActAdapter) -> Result<ActResponse, AdapterCallError> {
        if endpoint.manifest().adapter_id != self.invocation.capability.adapter_id {
            return Err(AdapterCallError::Rejected(
                "Act endpoint identity mismatch".into(),
            ));
        }
        match &self.fence {
            Some(fence) => endpoint.execute_fenced(&self.invocation, fence),
            None => endpoint.execute(&self.invocation),
        }
    }
}

/// Führt ausschließlich einen bereits materialisierten Action-Auftrag aus.
pub struct ExecutionService<'a> {
    policy: &'a AdapterRegistry,
    schemas: &'a SchemaRegistry,
    store: &'a SqliteArtifactStore,
    ids: &'a mut dyn ArtifactIdGenerator,
    clock: &'a dyn Clock,
}

impl<'a> ExecutionService<'a> {
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

    pub fn execute(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
    ) -> Result<Artifact, ExecutionError> {
        self.execute_inner(endpoint, action_version, None)
    }

    pub fn execute_reserved(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        reservation: &Reservation,
    ) -> Result<Artifact, ExecutionError> {
        self.execute_inner(endpoint, action_version, Some(reservation))
    }

    fn execute_inner(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        reservation: Option<&Reservation>,
    ) -> Result<Artifact, ExecutionError> {
        match self.prepare(endpoint, action_version, reservation)? {
            ExecutionPreparation::Complete(result) => Ok(*result),
            ExecutionPreparation::Dispatch(prepared) => {
                let response = prepared.call().send(endpoint);
                self.finish(*prepared, response)
            }
        }
    }

    /// Register the durable invocation and fence before the endpoint receives a call.
    pub fn prepare(
        &self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        reservation: Option<&Reservation>,
    ) -> Result<ExecutionPreparation, ExecutionError> {
        let action = self
            .store
            .get(action_version)?
            .ok_or_else(|| ExecutionError::MissingAction(action_version.clone()))?;
        if action.schema_id != SchemaId(ACTION_SCHEMA_ID.into()) {
            return Err(ExecutionError::NotActionArtifact);
        }
        if action.created_by.actor_type != ActorType::System
            || action.source.kind != SourceKind::Internal
        {
            return Err(ExecutionError::UntrustedActionArtifact);
        }
        let materialized: MaterializedActionPayload =
            serde_json::from_value(action.payload.clone())?;
        let candidate_version = VersionId(materialized.target_version);
        let approval_version = VersionId(materialized.approval_version);

        let candidate = self
            .store
            .get(&candidate_version)?
            .ok_or_else(|| ExecutionError::MissingCandidate(candidate_version.clone()))?;
        if candidate.artifact_type != "candidate"
            && !candidate.artifact_type.ends_with("_candidate")
        {
            return Err(ExecutionError::ApprovalTargetIsNotCandidate);
        }
        if candidate.schema_id != materialized.input_schema_id {
            return Err(ExecutionError::ActionInputSchemaMismatch);
        }
        if candidate.payload != materialized.payload {
            return Err(ExecutionError::ActionPayloadMismatch);
        }

        let action_relations = self.store.outgoing_relations(action_version)?;
        if !action_relations.iter().any(|relation| {
            relation.to == candidate_version && relation.kind == relation_kinds::derived_from()
        }) {
            return Err(ExecutionError::MissingActionTargetRelation);
        }
        if !action_relations.iter().any(|relation| {
            relation.to == approval_version && relation.kind == relation_kinds::based_on()
        }) {
            return Err(ExecutionError::MissingActionApprovalRelation);
        }

        let approval = self
            .store
            .get(&approval_version)?
            .ok_or_else(|| ExecutionError::MissingApproval(approval_version.clone()))?;
        if approval.schema_id != SchemaId(APPROVAL_SCHEMA_ID.into()) {
            return Err(ExecutionError::NotApprovalArtifact);
        }
        if !matches!(
            approval.created_by.actor_type,
            ActorType::Human | ActorType::System
        ) {
            return Err(ExecutionError::UnauthorizedApprovalActor);
        }
        let gate: ApprovalGatePayload = serde_json::from_value(approval.payload.clone())?;
        if gate.decision != ApprovalDecision::Approved {
            return Err(ExecutionError::ApprovalNotApproved);
        }
        if gate.target_version != candidate.version_id.0 {
            return Err(ExecutionError::ApprovalTargetMismatch);
        }
        let verification_version = VersionId(gate.verification_report_version);
        let approval_relations = self.store.outgoing_relations(&approval_version)?;
        if !approval_relations.iter().any(|relation| {
            relation.to == candidate_version && relation.kind == relation_kinds::approves()
        }) {
            return Err(ExecutionError::MissingApprovalRelation);
        }
        if !approval_relations.iter().any(|relation| {
            relation.to == verification_version && relation.kind == relation_kinds::based_on()
        }) {
            return Err(ExecutionError::MissingVerificationBasisRelation);
        }

        let capability = materialized.capability;
        if endpoint.manifest().adapter_id != capability.adapter_id {
            return Err(ExecutionError::EndpointAdapterMismatch);
        }
        let (registered, descriptor) = self
            .policy
            .authorized_capability(&capability.adapter_id, &capability.capability_id)?;
        if registered.grant().producer_class != ProducerClass::Executor {
            return Err(ExecutionError::ExecutorClassRequired(
                capability.adapter_id.clone(),
            ));
        }
        let Some(contract) = descriptor.contract.act() else {
            return Err(ExecutionError::CapabilityIsNotAct(capability));
        };
        if !contract.idempotent {
            return Err(ExecutionError::ActCapabilityIsNotIdempotent(capability));
        }
        if contract.fenced_resource != reservation.map(|record| record.resource.as_str()) {
            return Err(ExecutionError::ReservationFenceMismatch(capability));
        }
        if !contract.accepts.contains(&materialized.input_schema_id) {
            return Err(ExecutionError::InputSchemaNotAccepted {
                capability: capability.clone(),
                schema: materialized.input_schema_id,
            });
        }
        if contract.emits.len() != 1 {
            return Err(ExecutionError::InvalidResultSchemaCount {
                actual: contract.emits.len(),
            });
        }
        let result_schema = contract.emits[0].clone();
        let result_definition = self
            .schemas
            .get(&result_schema)
            .ok_or_else(|| ExecutionError::MissingRegisteredSchema(result_schema.clone()))?
            .clone();
        let required_tag = format!(
            "requires:{}:{}",
            capability.adapter_id.0, capability.capability_id.0
        );
        if !candidate.tags.iter().any(|tag| tag == &required_tag) {
            return Err(ExecutionError::CapabilityNotRequired(capability));
        }

        let grant = registered.grant();
        let invocation_id = execution_invocation_id(action_version, &capability, &result_schema);
        let invocation = ActInvocation {
            invocation_id: invocation_id.clone(),
            capability: capability.clone(),
            action_version_id: action_version.clone(),
            action_schema_id: action.schema_id.clone(),
            payload: action.payload.clone(),
            result_schema_id: result_schema.clone(),
        };
        let fence = reservation
            .map(|expected| {
                ReservationService::new(self.store, self.schemas, self.clock).act_fence(expected)
            })
            .transpose()
            .map_err(ExecutionError::Reservation)?;
        let serialized_invocation = match &fence {
            Some(fence) => serde_json::to_string(&(&invocation, fence)),
            None => serde_json::to_string(&invocation),
        }?;
        let dispatched = {
            let invocations = InvocationService::new(self.store, self.schemas, self.clock);
            let prepared = invocations.prepare(InvocationSpec {
                invocation_id: invocation_id.clone(),
                kind: InvocationKind::Execution,
                capability: capability_name(&capability),
                input_version: action_version.clone(),
                input_fingerprint: deterministic_input_fingerprint(&[&serialized_invocation]),
            })?;
            if prepared.status == InvocationStatus::Succeeded {
                let result = prepared
                    .result_version
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                return self
                    .store
                    .get(&result)?
                    .map(|artifact| ExecutionPreparation::Complete(Box::new(artifact)))
                    .ok_or(crate::runtime::InvocationError::MissingResult)
                    .map_err(ExecutionError::from);
            }
            let recovered = invocations.recover(&prepared)?;
            invocations.dispatch(&recovered)?
        };

        if let Some(fence) = &fence
            && let Err(error) = ReservationService::new(self.store, self.schemas, self.clock)
                .register_fenced_dispatch(fence, &capability)
        {
            let error = ExecutionError::Reservation(error);
            self.record_failure(&dispatched, &error)?;
            return Err(error);
        }
        Ok(ExecutionPreparation::Dispatch(Box::new(
            PreparedExecution {
                call: ExecutionCall { invocation, fence },
                dispatched,
                grant: grant.clone(),
                result_definition,
                approval_version,
                candidate_version,
            },
        )))
    }

    pub fn finish(
        &mut self,
        prepared: PreparedExecution,
        response: Result<ActResponse, AdapterCallError>,
    ) -> Result<Artifact, ExecutionError> {
        let PreparedExecution {
            call: ExecutionCall { invocation, fence },
            dispatched,
            grant,
            result_definition,
            approval_version,
            candidate_version,
        } = prepared;
        let ActInvocation {
            invocation_id,
            capability,
            action_version_id,
            result_schema_id,
            ..
        } = invocation;
        let outcome = (|| {
            let response = response?;
            if response.invocation_id != invocation_id {
                return Err(ExecutionError::InvocationResponseMismatch);
            }
            if response.external_reference.trim().is_empty() {
                return Err(ExecutionError::InvalidExternalReference);
            }
            let reference_size = response.external_reference.len();
            if reference_size > grant.max_external_reference_bytes {
                return Err(ExecutionError::ExternalReferenceTooLarge {
                    actual: reference_size,
                    maximum: grant.max_external_reference_bytes,
                });
            }
            let payload_size = serde_json::to_vec(&response.result_payload)?.len();
            if payload_size > grant.max_payload_bytes {
                return Err(ExecutionError::PayloadTooLarge {
                    actual: payload_size,
                    maximum: grant.max_payload_bytes,
                });
            }
            self.schemas
                .validate(&result_schema_id, &response.result_payload)
                .map_err(ExecutionError::InvalidPayload)?;

            let mut parents = vec![action_version_id.0.clone()];
            let mut relations = vec![(action_version_id.clone(), relation_kinds::result_of())];
            if let Some(fence) = &fence {
                parents.push(fence.reservation_version_id.0.clone());
                relations.push((
                    fence.reservation_version_id.clone(),
                    relation_kinds::based_on(),
                ));
            }
            let mut factory = ArtifactFactory::new(self.clock, self.ids);
            let artifact = factory.create(ArtifactFactoryInput {
                schema: result_definition,
                created_by: Actor {
                    actor_type: ActorType::Executor,
                    id: capability.adapter_id.0.clone(),
                },
                source: Source {
                    kind: SourceKind::External,
                    reference: response.external_reference,
                },
                trust: Trust {
                    level: grant.assigned_trust,
                    source_class: SourceClass::External,
                },
                stream_key: format!("execution:{invocation_id}"),
                subject: SubjectId(format!("execution:{}", action_version_id.0)),
                tags: vec![
                    format!("adapter:{}", capability.adapter_id.0),
                    format!("capability:{}", capability.capability_id.0),
                    format!("action:{}", action_version_id.0),
                    format!("approval:{}", approval_version.0),
                    format!("candidate:{}", candidate_version.0),
                ],
                payload: response.result_payload,
                provenance: Some(Provenance {
                    parents,
                    rules_applied: vec![
                        "execution.action_validated".into(),
                        "execution.capability_authorized".into(),
                        "execution.response_correlated".into(),
                        "execution.result_validated".into(),
                    ],
                    models_used: vec![],
                    transform: Some(format!("act_adapter:{}", capability.adapter_id.0)),
                }),
            })?;
            InvocationService::new(self.store, self.schemas, self.clock).succeed_with_event(
                &dispatched,
                &artifact,
                &relations,
            )?;
            Ok(artifact)
        })();

        if let Err(error) = &outcome {
            self.record_failure(&dispatched, error)?;
        }
        outcome
    }

    fn record_failure(
        &self,
        dispatched: &InvocationState,
        error: &ExecutionError,
    ) -> Result<(), ExecutionError> {
        InvocationService::new(self.store, self.schemas, self.clock).fail(
            dispatched,
            format!("{error:?}"),
            is_retryable(error),
        )?;
        Ok(())
    }
}

/// Stable execution identity for an approved action and its declared result schema.
pub fn execution_invocation_id(
    action: &VersionId,
    capability: &CapabilityRef,
    result_schema: &SchemaId,
) -> String {
    deterministic_invocation_id(
        InvocationKind::Execution,
        &[
            &action.0,
            &capability.adapter_id.0,
            &capability.capability_id.0,
            &result_schema.0,
        ],
    )
}

fn capability_name(capability: &CapabilityRef) -> String {
    format!("{}/{}", capability.adapter_id.0, capability.capability_id.0)
}

fn is_retryable(error: &ExecutionError) -> bool {
    matches!(
        error,
        ExecutionError::AdapterCall(
            crate::adapters::AdapterCallError::Unavailable(_)
                | crate::adapters::AdapterCallError::Timeout
        )
    )
}

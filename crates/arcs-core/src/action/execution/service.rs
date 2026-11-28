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
use crate::runtime::effects::{DispatchDecision, EffectDraft, EffectService, EffectState};
use crate::runtime::reservations::{Reservation, ReservationError, ReservationService};
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

    /// Führt einen materialisierten Auftrag aus.
    ///
    /// `task_key` benennt die Aufgabe, die die Wirkung verantwortet. Er
    /// bindet die externe Wirkung dauerhaft an ihren Auftrag, damit ein
    /// verspätetes Ergebnis nicht einer anderen Aufgabe zugerechnet wird.
    pub fn execute(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        task_key: &str,
    ) -> Result<Artifact, ExecutionError> {
        self.execute_inner(endpoint, action_version, task_key, None)
    }

    pub fn execute_reserved(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        task_key: &str,
        reservation: &Reservation,
    ) -> Result<Artifact, ExecutionError> {
        self.execute_inner(endpoint, action_version, task_key, Some(reservation))
    }

    fn execute_inner(
        &mut self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        task_key: &str,
        reservation: Option<&Reservation>,
    ) -> Result<Artifact, ExecutionError> {
        match self.prepare_inner(endpoint, action_version, task_key, reservation, true)? {
            ExecutionPreparation::Complete(result) => Ok(*result),
            ExecutionPreparation::Dispatch(prepared) => {
                let response = prepared.call().send(endpoint);
                self.finish(*prepared, response)
            }
        }
    }

    /// Register the durable invocation and fence before the endpoint receives a call.
    /// An interrupted dispatch can be replayed with its original identity.
    /// A recorded failure is returned to the task scheduler for reconciliation.
    pub fn prepare(
        &self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        task_key: &str,
        reservation: Option<&Reservation>,
    ) -> Result<ExecutionPreparation, ExecutionError> {
        self.prepare_inner(endpoint, action_version, task_key, reservation, false)
    }

    fn prepare_inner(
        &self,
        endpoint: &dyn ActAdapter,
        action_version: &VersionId,
        task_key: &str,
        reservation: Option<&Reservation>,
        retry_failed: bool,
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
        let operation_id = execution_operation_id(&invocation_id);
        let effects = EffectService::new(self.store, self.schemas, self.clock);
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
                // The effect belongs to the task that caused it. Another task
                // may not claim its result as its own.
                if let Some(record) = effects.lookup(&operation_id)?
                    && record.task_key != task_key
                {
                    return Err(ExecutionError::EffectBindingMismatch {
                        operation_id: operation_id.clone(),
                        expected: record.task_key,
                        actual: task_key.to_string(),
                    });
                }
                let result = prepared
                    .result_version
                    .ok_or(crate::runtime::InvocationError::MissingResult)?;
                let artifact = self
                    .store
                    .get(&result)?
                    .ok_or(crate::runtime::InvocationError::MissingResult)
                    .map_err(ExecutionError::from)?;
                // The stored receipt is the evidence of the external effect.
                self.confirm_effect(&effects, &operation_id, &artifact)?;
                return Ok(ExecutionPreparation::Complete(Box::new(artifact)));
            }
            // The book of external effects decides whether the effect may be
            // sent again. An unresolved effect is never repeated silently.
            let decision = self.authorize_effect(&effects, &operation_id)?;
            if prepared.status == InvocationStatus::Failed && !retry_failed {
                return Err(ExecutionError::RecordedFailure {
                    invocation_id,
                    detail: prepared
                        .error
                        .unwrap_or_else(|| "Act invocation failed".into()),
                });
            }
            // The effect is booked before the endpoint sees the call. A crash
            // after this point therefore leaves a durable trace. The binding
            // to the task is checked before anything becomes visible.
            effects.plan(&EffectDraft {
                operation_id: operation_id.clone(),
                task_key: task_key.to_string(),
                kind: format!("act:{}", capability.adapter_id.0),
                invocation_id: invocation_id.clone(),
            })?;
            let recovered = invocations.recover(&prepared)?;
            // Recovering the receipt of an effect that already happened must
            // not book a second dispatch. Only the attempt counter grows.
            match decision {
                DispatchDecision::Send => {
                    effects.mark_dispatched(&operation_id, None)?;
                }
                DispatchDecision::Replay { .. } => {
                    // The identical idempotent call is repeated. The attempt
                    // counter makes the repetition visible in the history.
                    effects.mark_replayed(&operation_id, None)?;
                }
                _ => {}
            }
            invocations.dispatch(&recovered)?
        };

        if let Some(fence) = &fence
            && let Err(error) = ReservationService::new(self.store, self.schemas, self.clock)
                .register_fenced_dispatch(fence, &capability)
        {
            let error = ExecutionError::Reservation(error);
            if !matches!(
                error,
                ExecutionError::Reservation(ReservationError::Store(_))
            ) {
                self.record_failure(&dispatched, &error)?;
            }
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
        // The external reference is read before the response is consumed. It
        // proves that the effect happened even when the receipt itself cannot
        // be written. A later attempt then only recovers the receipt instead
        // of causing a second effect.
        let received = response.as_ref().ok().and_then(|act| {
            (act.invocation_id == invocation_id && !act.external_reference.trim().is_empty())
                .then(|| act.external_reference.clone())
        });
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
            Ok((artifact, relations))
        })();

        let operation_id = execution_operation_id(&invocation_id);
        let effects = EffectService::new(self.store, self.schemas, self.clock);
        let (artifact, relations) = match outcome {
            Ok(result) => result,
            Err(error) => {
                self.record_failure(&dispatched, &error)?;
                self.record_effect_outcome(&effects, &operation_id, &error, received.as_deref())?;
                return Err(error);
            }
        };
        // A publication error leaves the invocation dispatched. Its external
        // effect may already exist, so recovery must use the identical call.
        if let Err(error) = InvocationService::new(self.store, self.schemas, self.clock)
            .succeed_with_event(&dispatched, &artifact, &relations)
        {
            // The adapter answered, so the effect happened even though the
            // receipt was lost. Recording that fact keeps a later attempt
            // from repeating the effect.
            self.confirm_effect(&effects, &operation_id, &artifact)?;
            return Err(ExecutionError::Invocation(error));
        }
        // The receipt is the evidence of the external effect.
        self.confirm_effect(&effects, &operation_id, &artifact)?;
        Ok(artifact)
    }

    /// Prüft, ob die externe Wirkung erneut abgesendet werden darf.
    ///
    /// Eine Wirkung, die nicht stattgefunden hat, darf gesendet werden. Eine
    /// bestätigte Wirkung wird nicht wiederholt: Der erneute Aufruf holt nur
    /// ihren Beleg zurück. Eine ungeklärte Wirkung wird in den Zustand
    /// `unclear` überführt und verlangt eine ausdrückliche Klärung, bevor
    /// erneut gesendet werden darf.
    fn authorize_effect(
        &self,
        effects: &EffectService<'_>,
        operation_id: &str,
    ) -> Result<DispatchDecision, ExecutionError> {
        match effects.authorize_dispatch(operation_id)? {
            DispatchDecision::Send => Ok(DispatchDecision::Send),
            DispatchDecision::Replay { attempt } => Ok(DispatchDecision::Replay { attempt }),
            DispatchDecision::RecoverEvidence { .. } => {
                // The effect happened; only its receipt is missing. The repeated
                // call is the identical idempotent call and recovers the
                // evidence without causing a second effect.
                effects.mark_evidence_recovery(operation_id)?;
                Ok(DispatchDecision::RecoverEvidence {
                    reference: String::new(),
                })
            }
            DispatchDecision::NeedsReconciliation { detail } => {
                Err(ExecutionError::UnclearExternalEffect {
                    operation_id: operation_id.to_string(),
                    detail,
                })
            }
        }
    }

    /// Bestätigt eine externe Wirkung mit ihrem Beleg.
    fn confirm_effect(
        &self,
        effects: &EffectService<'_>,
        operation_id: &str,
        artifact: &Artifact,
    ) -> Result<(), ExecutionError> {
        let Some(record) = effects.lookup(operation_id)? else {
            return Ok(());
        };
        if record.state == EffectState::Confirmed {
            return Ok(());
        }
        effects.confirm(operation_id, &artifact.source.reference)?;
        Ok(())
    }

    /// Hält fest, was über eine fehlgeschlagene externe Wirkung bekannt ist.
    ///
    /// Ein Adapter, der nicht erreicht wurde oder die Anfrage abgelehnt hat,
    /// belegt: Es ist nichts passiert und ein neuer Versuch ist zulässig. Ein
    /// Timeout, ein Protokollfehler oder ein unbrauchbares Ergebnis belegt
    /// nichts: Die Wirkung bleibt ungeklärt und wird nicht wiederholt.
    ///
    /// Hat der Adapter mit einer passenden Referenz geantwortet, ist die
    /// Wirkung eingetreten. Ein fehlender Beleg darf dann nicht zu einer
    /// zweiten Wirkung führen.
    fn record_effect_outcome(
        &self,
        effects: &EffectService<'_>,
        operation_id: &str,
        error: &ExecutionError,
        received: Option<&str>,
    ) -> Result<(), ExecutionError> {
        let Some(record) = effects.lookup(operation_id)? else {
            return Ok(());
        };
        if record.state == EffectState::Confirmed {
            return Ok(());
        }
        let detail = format!("{error:?}");
        if let Some(reference) = received {
            effects.confirm(operation_id, reference)?;
            return Ok(());
        }
        match error {
            ExecutionError::AdapterCall(
                crate::adapters::AdapterCallError::Unavailable(_)
                | crate::adapters::AdapterCallError::Rejected(_),
            ) => {
                // The call did not reach the effect. A new attempt is allowed.
                effects.mark_failed(operation_id, &detail)?;
            }
            ExecutionError::InvocationResponseMismatch
            | ExecutionError::InvalidExternalReference => {
                // The answer cannot be attributed to this operation. Only an
                // explicit clarification may continue the task.
                effects.mark_unclear(operation_id, &detail)?;
            }
            _ => {
                // A timeout or a broken protocol leaves the outcome unknown.
                // The effect stays replayable: the identical idempotent call
                // may be repeated, and the attempt stays visible.
            }
        }
        Ok(())
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

/// Stable identity of the external effect of an approved action.
///
/// The effect follows the invocation. A replay of the same approved action
/// therefore refers to the same effect and can be recognised as such.
pub fn execution_operation_id(invocation_id: &str) -> String {
    format!("operation:{invocation_id}")
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

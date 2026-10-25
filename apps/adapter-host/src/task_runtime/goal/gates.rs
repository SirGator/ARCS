use arcs::action::ActionService;
use arcs::approval::{ApprovalDecision, ApprovalService};
use arcs::core::{Actor, ActorType, Artifact, SystemClock, VersionId};
use arcs::outcome::{OutcomeError, OutcomeEvaluator, OutcomeResult, OutcomeService};
use arcs::store::{ArtifactRelations, relation_kinds};
use arcs::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

use super::engine::load;
use super::*;
use crate::task_runtime::TaskRuntime;

struct Gate {
    result: VerificationResult,
}
impl Verifier for Gate {
    fn verify(&self, _: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(self.result.clone())
    }
}
struct Effect {
    result: OutcomeResult,
}
impl OutcomeEvaluator for Effect {
    fn evaluate(&self, _: &Artifact) -> Result<OutcomeResult, OutcomeError> {
        Ok(self.result.clone())
    }
}
fn service_error(error: impl std::fmt::Debug) -> HostError {
    invalid(format!("goal gate: {error:?}"))
}

impl TaskRuntime {
    pub(super) fn recover_goal_review(
        &self,
        host: &Host,
        task: &mut GoalTask,
    ) -> Result<(), HostError> {
        let candidate = task
            .candidate
            .as_ref()
            .ok_or_else(|| invalid("missing review candidate"))?;
        if let Some(approval) = host
            .store()
            .find_by_stream_key(&format!("approval:{}", candidate.0))
            .map_err(HostError::Store)?
        {
            if approval.schema_id.0 != "arcs.approval.v1"
                || approval.payload["target_version"] != candidate.0
                || !matches!(
                    approval.created_by.actor_type,
                    ActorType::Human | ActorType::System
                )
                || task.verification.as_ref().is_none_or(|version| {
                    approval.payload["verification_report_version"] != version.0
                })
            {
                return Err(invalid("saved approval does not match goal review"));
            }
            task.state = match approval.payload["decision"].as_str() {
                Some("approved") => GoalState::Executing,
                Some("rejected") => GoalState::Rejected,
                _ => return Err(invalid("invalid saved approval decision")),
            };
            task.approval = Some(approval.version_id);
        }
        Ok(())
    }
    fn gate(
        &self,
        host: &Host,
        route: &ReasoningRoute,
        config: &GoalConfig,
        task: &GoalTask,
    ) -> Result<Gate, HostError> {
        let candidate = load(
            host,
            task.candidate
                .as_ref()
                .ok_or_else(|| invalid("missing candidate"))?,
        )?;
        let capability = task
            .capability
            .as_ref()
            .ok_or_else(|| invalid("missing action capability"))?;
        let rule = config
            .actions
            .iter()
            .find(|action| &action.capability == capability)
            .ok_or_else(|| invalid("action not configured"))?;
        host.registry()
            .authorized_capability(&capability.adapter_id, &capability.capability_id)
            .map_err(HostError::Registration)?;
        let parents = candidate.provenance.as_ref().map(|p| &p.parents);
        let passed = candidate.schema_id == route.target_schema_id
            && candidate.created_by.id == route.reasoner.adapter_id.0
            && candidate.tags.contains(&format!(
                "requires:{}:{}",
                capability.adapter_id.0, capability.capability_id.0
            ))
            && parents.is_some_and(|parents| {
                parents.contains(&task.input_version_id.0)
                    && task.before.iter().all(|v| parents.contains(&v.0))
            })
            && host
                .schemas()
                .validate(&rule.verification_schema_id, &candidate.payload)
                .is_ok();
        let verdict = if passed {
            VerificationVerdict::Pass
        } else {
            VerificationVerdict::Fail
        };
        Ok(Gate {
            result: VerificationResult {
                verdict,
                findings: vec![VerificationFinding {
                    check: "goal.configured_action_contract".into(),
                    verdict,
                    detail: format!(
                        "check planner origin, selected goal evidence, capability and operator payload contract {}",
                        rule.verification_schema_id.0
                    ),
                }],
            },
        })
    }

    pub(super) fn verify_goal(
        &mut self,
        host: &Host,
        route: &ReasoningRoute,
        config: &GoalConfig,
        task: &mut GoalTask,
    ) -> Result<(), HostError> {
        let gate = self.gate(host, route, config, task)?;
        let candidate = task.candidate.as_ref().unwrap();
        let stream = format!("verification:{}", candidate.0);
        let verification = match host
            .store()
            .find_by_stream_key(&stream)
            .map_err(HostError::Store)?
        {
            Some(artifact) => artifact,
            None => VerificationService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .verify(candidate, &gate)
            .map_err(service_error)?,
        };
        if verification.created_by.id != "arcs.verification"
            || verification.payload["target_version"] != candidate.0
        {
            return Err(invalid(
                "verification belongs to another target or producer",
            ));
        }
        task.verification = Some(verification.version_id);
        if gate.result.verdict != VerificationVerdict::Pass
            || verification.payload["verdict"] != "pass"
        {
            task.state = GoalState::Blocked;
            task.detail = "action failed the configured verification contract".into();
        } else {
            task.state = GoalState::AwaitingApproval;
            if let GoalApproval::Policy { policy_id } = &config.approval {
                self.decide_goal(
                    host,
                    task,
                    ApprovalDecision::Approved,
                    Actor {
                        actor_type: ActorType::System,
                        id: policy_id.clone(),
                    },
                    format!(
                        "explicit operator policy {policy_id}; verified configured action contract"
                    ),
                )?;
            }
        }
        Ok(())
    }

    fn decide_goal(
        &mut self,
        host: &Host,
        task: &mut GoalTask,
        decision: ApprovalDecision,
        actor: Actor,
        reason: String,
    ) -> Result<(), HostError> {
        let candidate = task
            .candidate
            .as_ref()
            .ok_or_else(|| invalid("missing candidate"))?;
        let verification = task
            .verification
            .as_ref()
            .ok_or_else(|| invalid("missing verification"))?;
        let approval = match host
            .store()
            .find_by_stream_key(&format!("approval:{}", candidate.0))
            .map_err(HostError::Store)?
        {
            Some(approval) => {
                let expected = serde_json::to_value(decision).map_err(service_error)?;
                if approval.payload["decision"] != expected
                    || approval.created_by != actor
                    || approval.payload["verification_report_version"] != verification.0
                {
                    return Err(invalid(
                        "an immutable decision already exists for this candidate",
                    ));
                }
                approval
            }
            None => ApprovalService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .decide(candidate, verification, decision, actor, reason)
            .map_err(service_error)?,
        };
        task.approval = Some(approval.version_id);
        task.state = if decision == ApprovalDecision::Approved {
            GoalState::Executing
        } else {
            GoalState::Rejected
        };
        Ok(())
    }

    /// Vertrauenswürdiger Anwendungseinstieg. Der Aufrufer authentifiziert den
    /// Menschen/die Systempolicy; ein Reasoner kann diesen Port nicht aufrufen.
    /// Die konkrete Candidate-Version ist Pflicht, damit keine neuere Runde
    /// durch eine verspätete Entscheidung freigegeben wird.
    pub fn review_goal(
        &mut self,
        host: &Host,
        sequence: u64,
        candidate: &VersionId,
        decision: ApprovalDecision,
        actor: Actor,
        reason: String,
    ) -> Result<(), HostError> {
        self.check_goal_host(host)?;
        if reason.trim().is_empty()
            || reason.len() > 2000
            || actor.id.trim().is_empty()
            || actor.id.len() > 128
            || !matches!(actor.actor_type, ActorType::Human | ActorType::System)
        {
            return Err(invalid(
                "review needs a human/system actor and a bounded reason",
            ));
        }
        let mut task = self
            .saved_goal(sequence)?
            .ok_or_else(|| invalid("unknown goal"))?;
        if task.state != GoalState::AwaitingApproval || task.candidate.as_ref() != Some(candidate) {
            return Err(invalid("goal is not waiting for this candidate's approval"));
        }
        let route = self
            .config
            .routes
            .iter()
            .find(|r| r.id == task.route_id)
            .ok_or_else(|| invalid("missing route"))?
            .clone();
        let config = route
            .goal
            .as_ref()
            .ok_or_else(|| invalid("not a goal route"))?;
        if decision == ApprovalDecision::Approved
            && self.gate(host, &route, config, &task)?.result.verdict != VerificationVerdict::Pass
        {
            return Err(invalid("current action verification failed"));
        }
        self.decide_goal(host, &mut task, decision, actor, reason)?;
        self.save_goal(&task)
    }

    pub fn cancel_goal(&self, host: &Host, sequence: u64, reason: String) -> Result<(), HostError> {
        self.check_goal_host(host)?;
        if reason.trim().is_empty() || reason.len() > 2000 {
            return Err(invalid("invalid cancellation reason"));
        }
        let mut task = self
            .saved_goal(sequence)?
            .ok_or_else(|| invalid("unknown goal"))?;
        if task.state.terminal() {
            return Err(invalid("goal already ended"));
        }
        // After dispatch an execution may already exist despite a lost checkpoint.
        // Cancellation stops further work; it never claims to roll back an effect.
        task.state = GoalState::Cancelled;
        task.detail = reason;
        self.save_goal(&task)
    }

    fn check_goal_host(&self, host: &Host) -> Result<(), HostError> {
        if host.database_path() != self.core_database
            || self.config.identity(host)? != self.identity
        {
            return Err(invalid("runtime topology changed"));
        }
        Ok(())
    }

    pub(super) fn execute_goal(
        &mut self,
        host: &mut Host,
        route: &ReasoningRoute,
        config: &GoalConfig,
        task: &mut GoalTask,
    ) -> Result<(), HostError> {
        // Revalidate policy on resume as well. Core independently checks approval,
        // schema, capability grant and idempotent execution before dispatch.
        if self.gate(host, route, config, task)?.result.verdict != VerificationVerdict::Pass {
            return Err(invalid("action no longer passes verification"));
        }
        let approval = task
            .approval
            .as_ref()
            .ok_or_else(|| invalid("missing approval"))?;
        let capability = task
            .capability
            .as_ref()
            .ok_or_else(|| invalid("missing action capability"))?;
        let action = ActionService::new(
            host.registry(),
            host.schemas(),
            host.store(),
            &mut self.journal.ids,
            &SystemClock,
        )
        .materialize(approval, capability)
        .map_err(service_error)?;
        let execution = host.execute_approved(&capability.adapter_id, &action.version_id)?;
        task.action = Some(action.version_id);
        task.execution = Some(execution.version_id);
        Ok(())
    }

    pub(super) fn record_goal_outcome(
        &mut self,
        host: &Host,
        task: &GoalTask,
        assessment: &GoalAssessment,
    ) -> Result<VersionId, HostError> {
        let execution = task
            .execution
            .as_ref()
            .ok_or_else(|| invalid("missing execution"))?;
        let detail = format!("external assessment: {}", assessment.detail);
        let outcome = match host
            .store()
            .find_by_stream_key(&format!("outcome:{}", execution.0))
            .map_err(HostError::Store)?
        {
            Some(outcome) => {
                if outcome.payload["verdict"]
                    != serde_json::to_value(assessment.effect).map_err(service_error)?
                    || outcome.payload["detail"] != detail
                {
                    return Err(invalid("outcome conflicts with saved assessment"));
                }
                outcome
            }
            None => OutcomeService::new(
                host.schemas(),
                host.store(),
                &mut self.journal.ids,
                &SystemClock,
            )
            .evaluate(
                execution,
                &Effect {
                    result: OutcomeResult {
                        verdict: assessment.effect,
                        detail,
                    },
                },
            )
            .map_err(service_error)?,
        };
        let relations = ArtifactRelations::new(host.store());
        for evidence in assessment
            .evidence_version_ids
            .iter()
            .chain(task.assessment.iter())
        {
            if !relations
                .outgoing(&outcome.version_id)
                .map_err(HostError::Store)?
                .iter()
                .any(|r| &r.to == evidence && r.kind == relation_kinds::supported_by())
            {
                relations
                    .connect(
                        outcome.version_id.clone(),
                        evidence.clone(),
                        relation_kinds::supported_by(),
                    )
                    .map_err(HostError::Store)?;
            }
        }
        Ok(outcome.version_id)
    }
}

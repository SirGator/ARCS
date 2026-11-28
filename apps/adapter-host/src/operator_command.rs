//! Commands from the trusted host application, shared by both CLI entry points.
use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, VersionId};
use serde::{Deserialize, Serialize};

use crate::task_runtime::effects::{EffectOutcome, EffectReceipt};
use crate::task_runtime::formal::{FormalEvidenceRequirement, FormalObligationRequest};
use crate::task_runtime::output::{OutputCommand, OutputDelivery};
use crate::task_runtime::queue::{TaskQueuePage, TaskQueueQuery};
use crate::task_runtime::scheduler::{SeriesControlCommand, SeriesControlReceipt};
use crate::task_runtime::suspension::{TaskControlCommand, TaskControlReceipt};
use crate::task_runtime::task_output::{TaskOutputCommand, TaskOutputResponse};
use crate::task_runtime::{ControlReleaseIntent, ResourceEvidenceReview, goal::GoalReview};
use crate::{Host, HostError, task_runtime::TaskRuntime};

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanCommand {
    Cancel { sequence: u64, reason: String },
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalCommand {
    Inspect {
        sequence: u64,
        candidate_version_id: VersionId,
    },
    InspectResourceEvidence {
        sequence: u64,
        reservation_versions: Vec<VersionId>,
        evidence_versions: Vec<VersionId>,
    },
    ReleaseResources {
        sequence: u64,
        reservation_versions: Vec<VersionId>,
        evidence_versions: Vec<VersionId>,
        operator_id: String,
        reason: String,
    },
    Review {
        sequence: u64,
        candidate_version_id: VersionId,
        decision: ApprovalDecision,
        operator_id: String,
        reason: String,
    },
    Cancel {
        sequence: u64,
        reason: String,
    },
}

/// Befehle der formalen Nachweise.
///
/// Die Befehle kommen aus der vertrauenswürdigen Anwendung. Ein Reasoner kann
/// sie nicht auslösen: Sie legen Nachweise an, die eine Handlung tragen.
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormalCommand {
    /// Legt eine formale Obligation samt Definitionen und Annahmen an.
    Obligate { request: FormalObligationRequest },
    /// Stößt die formale Prüfung einer Obligation an.
    Prove {
        obligation: String,
        requirement: FormalEvidenceRequirement,
    },
}

/// Befehle des Buchs der externen Wirkungen.
///
/// Die Befehle kommen aus der vertrauenswürdigen Anwendung. Eine ungeklärte
/// Wirkung wird nicht wiederholt: Nur die ausdrückliche Klärung des
/// Betreibers löst die Aufgabe aus dem Wartezustand.
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectCommand {
    /// Zeigt den Stand einer externen Wirkung.
    Inspect { operation_id: String },
    /// Klärt eine ungeklärte externe Wirkung ausdrücklich.
    Reconcile {
        operation_id: String,
        outcome: EffectOutcome,
        /// Beleg der eingetretenen Wirkung.
        #[serde(default)]
        reference: Option<String>,
        reason: String,
        operator_id: String,
    },
}

/// Befehle des Geheimnistresors.
///
/// Die Befehle kommen aus der vertrauenswürdigen Anwendung. Der Wert eines
/// Geheimnisses wird nur beim Anlegen übergeben und verlässt den Tresor
/// danach nicht mehr.
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretCommand {
    /// Nimmt ein Geheimnis in den Tresor auf und gibt seine Kennung zurück.
    Register { value: String },
    /// Stellt eine Freigabe für ein Geheimnis aus.
    Issue {
        secret_id: String,
        parameter: String,
        purpose: String,
        resource: String,
        ttl_seconds: u64,
        max_uses: u32,
    },
    /// Zeigt den Stand einer Freigabe.
    Inspect { grant_id: String },
    /// Widerruft eine Freigabe ausdrücklich.
    Revoke { grant_id: String },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperatorCommand {
    Queue(TaskQueueQuery),
    Task(TaskControlCommand),
    Series(SeriesControlCommand),
    Plan(PlanCommand),
    Goal(GoalCommand),
    Output(OutputCommand),
    TaskOutput(TaskOutputCommand),
    Formal(FormalCommand),
    Effect(EffectCommand),
    Secret(SecretCommand),
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum CommandReceipt {
    Queue(TaskQueuePage),
    Task(TaskControlReceipt),
    Series(SeriesControlReceipt),
    Output(OutputDelivery),
    TaskOutput(TaskOutputResponse),
    GoalReview(GoalReview),
    ResourceEvidenceReview(ResourceEvidenceReview),
    ResourceRelease {
        sequence: u64,
        release_pending: bool,
    },
    Formal(FormalReceipt),
    Effect(EffectReceipt),
    Secret(SecretReceipt),
}

/// Der Nachweis über eine Geheimnisfreigabe.
///
/// Der Bericht nennt Zweck, Ressource, Ablauf und Nutzungen. Er enthält nie
/// den Wert eines Geheimnisses.
#[derive(Serialize)]
pub struct SecretReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

/// Der Nachweis über den Stand eines formalen Nachweises.
#[derive(Serialize)]
pub struct FormalReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obligation_version_id: Option<VersionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_version_id: Option<VersionId>,
    /// `pending`, wenn die Prüfung noch läuft.
    pub state: String,
}

impl OperatorCommand {
    pub fn apply(
        self,
        runtime: &mut TaskRuntime,
        host: &mut Host,
    ) -> Result<Option<CommandReceipt>, HostError> {
        self.apply_command(runtime, host, false)
    }

    pub fn apply_live(
        self,
        runtime: &mut TaskRuntime,
        host: &mut Host,
    ) -> Result<Option<CommandReceipt>, HostError> {
        self.apply_command(runtime, host, true)
    }

    fn apply_command(
        self,
        runtime: &mut TaskRuntime,
        host: &mut Host,
        live: bool,
    ) -> Result<Option<CommandReceipt>, HostError> {
        match self {
            Self::Queue(query) => runtime
                .task_queue(query)
                .map(|page| Some(CommandReceipt::Queue(page))),
            Self::Task(command) => runtime
                .control_task(host, command)
                .map(|receipt| Some(CommandReceipt::Task(receipt))),
            Self::Series(command) => runtime
                .control_check_series(host, command)
                .map(|receipt| Some(CommandReceipt::Series(receipt))),
            Self::Plan(PlanCommand::Cancel { sequence, reason }) => {
                runtime.cancel_plan(host, sequence, reason).map(|()| None)
            }
            Self::Goal(command) => command.apply(runtime, host, live),
            Self::Output(command) => runtime
                .control_output(host, command)
                .map(|receipt| Some(CommandReceipt::Output(receipt))),
            Self::TaskOutput(command) => runtime
                .control_task_output(host, command)
                .map(|receipt| Some(CommandReceipt::TaskOutput(receipt))),
            Self::Formal(command) => runtime
                .apply_formal(host, command)
                .map(|receipt| Some(CommandReceipt::Formal(receipt))),
            Self::Effect(command) => command.apply(runtime, host),
            Self::Secret(command) => command.apply(host),
        }
    }
}

impl SecretCommand {
    fn apply(self, host: &Host) -> Result<Option<CommandReceipt>, HostError> {
        match self {
            Self::Register { value } => {
                let secret_id = host.register_secret(&value)?;
                Ok(Some(CommandReceipt::Secret(SecretReceipt {
                    secret_id: Some(secret_id.as_str().to_string()),
                    ..SecretReceipt::empty()
                })))
            }
            Self::Issue {
                secret_id,
                parameter,
                purpose,
                resource,
                ttl_seconds,
                max_uses,
            } => {
                let grant = host.issue_secret_grant(&arcs::privacy::SecretGrantSpec {
                    secret_id: arcs::privacy::SecretId::new(secret_id),
                    parameter,
                    purpose,
                    resource,
                    ttl_seconds,
                    max_uses,
                })?;
                Ok(Some(CommandReceipt::Secret(SecretReceipt::of(&grant))))
            }
            Self::Inspect { grant_id } => {
                let receipt = match host.secret_grant(&grant_id)? {
                    Some(grant) => SecretReceipt::of(&grant),
                    None => SecretReceipt::empty(),
                };
                Ok(Some(CommandReceipt::Secret(receipt)))
            }
            Self::Revoke { grant_id } => {
                let grant = host.revoke_secret_grant(&grant_id)?;
                Ok(Some(CommandReceipt::Secret(SecretReceipt::of(&grant))))
            }
        }
    }
}

impl EffectCommand {
    fn apply(
        self,
        runtime: &mut TaskRuntime,
        host: &mut Host,
    ) -> Result<Option<CommandReceipt>, HostError> {
        match self {
            Self::Inspect { operation_id } => {
                let receipt = runtime.effect(host, &operation_id)?;
                Ok(receipt.map(CommandReceipt::Effect))
            }
            Self::Reconcile {
                operation_id,
                outcome,
                reference,
                reason,
                operator_id,
            } => {
                let receipt = runtime.reconcile_effect(
                    host,
                    &operation_id,
                    outcome,
                    reference.as_deref(),
                    &reason,
                    &operator_id,
                )?;
                Ok(Some(CommandReceipt::Effect(receipt)))
            }
        }
    }
}

impl GoalCommand {
    fn apply(
        self,
        runtime: &mut TaskRuntime,
        host: &Host,
        live: bool,
    ) -> Result<Option<CommandReceipt>, HostError> {
        match self {
            Self::Inspect {
                sequence,
                candidate_version_id,
            } => {
                return runtime
                    .inspect_goal_review(host, sequence, &candidate_version_id)
                    .map(|review| Some(CommandReceipt::GoalReview(review)));
            }
            Self::InspectResourceEvidence {
                sequence,
                reservation_versions,
                evidence_versions,
            } => {
                return runtime
                    .inspect_resource_evidence(
                        host,
                        sequence,
                        reservation_versions,
                        &evidence_versions,
                    )
                    .map(|review| Some(CommandReceipt::ResourceEvidenceReview(review)));
            }
            Self::ReleaseResources {
                sequence,
                reservation_versions,
                evidence_versions,
                operator_id,
                reason,
            } => {
                if live {
                    let intent = ControlReleaseIntent {
                        expected: reservation_versions,
                        evidence: evidence_versions,
                        actor: human(operator_id),
                        reason,
                    };
                    return runtime
                        .queue_goal_reconciliation(host, sequence, intent)
                        .map(|release_pending| {
                            Some(CommandReceipt::ResourceRelease {
                                sequence,
                                release_pending,
                            })
                        });
                }
                runtime.reconcile_goal_resources(
                    host,
                    sequence,
                    &reservation_versions,
                    &evidence_versions,
                    human(operator_id),
                    reason,
                )
            }
            Self::Review {
                sequence,
                candidate_version_id,
                decision,
                operator_id,
                reason,
            } => runtime.review_goal(
                host,
                sequence,
                &candidate_version_id,
                decision,
                human(operator_id),
                reason,
            ),
            Self::Cancel { sequence, reason } => runtime.cancel_goal(host, sequence, reason),
        }
        .map(|()| None)
    }
}

impl SecretReceipt {
    fn empty() -> Self {
        Self {
            secret_id: None,
            grant_id: None,
            state: None,
            expires_at: None,
            uses: None,
            max_uses: None,
            purpose: None,
            resource: None,
        }
    }

    fn of(grant: &arcs::privacy::SecretGrant) -> Self {
        Self {
            secret_id: Some(grant.secret_id.as_str().to_string()),
            grant_id: Some(grant.grant_id.clone()),
            state: Some(grant.state.as_str().to_string()),
            expires_at: Some(grant.expires_at.clone()),
            uses: Some(grant.uses),
            max_uses: Some(grant.max_uses),
            purpose: Some(grant.purpose.clone()),
            resource: Some(grant.resource.clone()),
        }
    }
}

fn human(id: String) -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id,
    }
}

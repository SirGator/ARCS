//! Commands from the trusted host application, shared by both CLI entry points.
use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, VersionId};
use serde::{Deserialize, Serialize};

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

fn human(id: String) -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id,
    }
}

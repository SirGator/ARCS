use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
    ProposalSubmission, ReasoningInvocationV2, ReasoningMethod, ReasoningResponseV2,
    ReasoningStatus, ReasoningTraceV2, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
const TASK: &str = "arcs.task.nearest.v1";
const CANDIDATE: &str = "arcs.solution_candidate.nearest.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    method: ReasoningMethod,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    values: Vec<f64>,
    target: f64,
}
struct Reasoner {
    profile: Profile,
    database: Connection,
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}
fn rejected(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::rejected(error.to_string())
}
impl Adapter for Reasoner {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: self.profile.adapter_id.clone(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId("solution.propose".into()),
                contract: CapabilityContract::Reason {
                    emits: vec![SchemaId(CANDIDATE.into())],
                },
                required_permissions: vec!["reason.compute".into()],
            }],
        }
    }
    fn reason_v2(
        &mut self,
        invocation: ReasoningInvocationV2,
    ) -> Result<ReasoningResponseV2, AdapterError> {
        if invocation.method != self.profile.method {
            return Err(rejected("reasoning method/profile mismatch"));
        }
        let input = serde_json::to_string(&invocation).map_err(rejected)?;
        let transaction = self
            .database
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let previous: Option<(String, String)> = transaction
            .query_row(
                "SELECT input,response FROM invocations WHERE id=?1",
                [&invocation.invocation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(unavailable)?;
        if let Some((old, response)) = previous {
            if old != input {
                return Err(rejected("reasoning invocation identity conflict"));
            }
            return serde_json::from_str(&response).map_err(unavailable);
        }
        let context = invocation
            .context
            .iter()
            .find(|item| item.schema_id.0 == TASK)
            .ok_or_else(|| rejected("missing task context"))?;
        let task: Task = serde_json::from_value(context.payload.clone()).map_err(rejected)?;
        if task.values.len() > 4096
            || !task.target.is_finite()
            || task
                .values
                .iter()
                .any(|value| !value.is_finite() || !(*value - task.target).abs().is_finite())
        {
            return Err(rejected("invalid finite search task"));
        }
        let solver = self.profile.method == ReasoningMethod::Solver;
        let exhausted = solver
            && invocation
                .budget
                .max_iterations
                .is_none_or(|max| task.values.len() as u64 > max);
        let status = if exhausted {
            ReasoningStatus::BudgetExhausted
        } else if task.values.is_empty() {
            ReasoningStatus::NoSolution
        } else {
            ReasoningStatus::Solved
        };
        let mut candidates = Vec::new();
        if status == ReasoningStatus::Solved {
            let value = if solver {
                *task
                    .values
                    .iter()
                    .min_by(|a, b| {
                        (**a - task.target)
                            .abs()
                            .total_cmp(&(**b - task.target).abs())
                    })
                    .unwrap()
            } else {
                task.values[0]
            };
            candidates.push(ProposalSubmission {
                schema_id: invocation.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: vec![context.version_id.clone()],
                payload: json!({"value":value,"distance":(value-task.target).abs()}),
            });
        }
        let response = ReasoningResponseV2 {
            invocation_id: invocation.invocation_id.clone(),
            request_id: invocation.request_id,
            status,
            candidates,
            trace: ReasoningTraceV2 {
                method: self.profile.method,
                implementation: if solver {
                    "finite-nearest-solver"
                } else {
                    "recorded-model-fixture"
                }
                .into(),
                version: env!("CARGO_PKG_VERSION").into(),
                iterations: if solver {
                    Some(if exhausted {
                        0
                    } else {
                        task.values.len() as u64
                    })
                } else {
                    None
                },
                output_tokens: None,
                details: json!({"fixture":!solver,"task_schema":TASK}),
            },
        };
        transaction
            .execute(
                "INSERT INTO invocations VALUES(?1,?2,?3)",
                params![
                    invocation.invocation_id,
                    input,
                    serde_json::to_string(&response).map_err(unavailable)?
                ],
            )
            .map_err(unavailable)?;
        transaction.commit().map_err(unavailable)?;
        Ok(response)
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: arcs-example-reasoner <profile.json> <journal.sqlite>".into());
    }
    let profile: Profile = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    if !matches!(
        profile.method,
        ReasoningMethod::Solver | ReasoningMethod::Learned
    ) {
        return Err("example supports solver or learned test fixture only".into());
    }
    let identity = serde_json::to_string(&profile)?;
    let mut database = Connection::open(&args[1])?;
    database.execute_batch("PRAGMA synchronous=FULL;CREATE TABLE IF NOT EXISTS installation(singleton INTEGER PRIMARY KEY CHECK(singleton=1),profile TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS invocations(id TEXT PRIMARY KEY,input TEXT NOT NULL,response TEXT NOT NULL);")?;
    let transaction = database.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT OR IGNORE INTO installation VALUES(1,?1)",
        [&identity],
    )?;
    let previous: String =
        transaction.query_row("SELECT profile FROM installation", [], |row| row.get(0))?;
    if previous != identity {
        return Err("reasoner journal profile identity conflict".into());
    }
    transaction.commit()?;
    arcs_adapter_sdk::serve(
        &mut Reasoner { profile, database },
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("example reasoner: {error}");
        std::process::exit(1);
    }
}

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use arcs::approval::ApprovalDecision;
use arcs::core::{Actor, ActorType, VersionId};
use arcs_host::Host;
use arcs_host::task_runtime::suspension::TaskControlCommand;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde::Deserialize;

const USAGE: &str = "usage: arcs-host --config <host.json> [--list-adapters | --find-capabilities <query.json> | --reasoning-attempts <query.json> | [--monitor --stop-file <path>] [--runtime <runtime.json>] [--ticks <count>] [--stop-file <path>] | --runtime <runtime.json> [--goal-command <command.json> | --plan-command <command.json> | --task-command <command.json>]]";

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum PlanCommand {
    Cancel { sequence: u64, reason: String },
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum GoalCommand {
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
fn run() -> Result<(), String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Pure contract validation for local input clients. No database, adapter
    // process or ingress event is opened before this branch returns.
    if args.len() == 2 && args[0] == "--validate-input" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct InputCheck {
            schema: serde_json::Value,
            payload: serde_json::Value,
        }
        let bytes = std::fs::read(&args[1]).map_err(|error| error.to_string())?;
        let check: InputCheck =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let schema_id = check.schema["$id"]
            .as_str()
            .ok_or("missing schema ID")?
            .to_owned();
        let mut schemas = arcs::core::SchemaRegistry::new();
        schemas
            .register_json(&check.schema.to_string())
            .map_err(|error| format!("{error:?}"))?;
        schemas
            .validate(&arcs::core::SchemaId(schema_id), &check.payload)
            .map_err(|errors| format!("Eingabe passt nicht zum Adaptervertrag: {errors:?}"))?;
        println!("{{\"valid\":true}}");
        return Ok(());
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        println!("       arcs-host --validate-input <schema-and-payload.json>");
        return Ok(());
    }
    let mut config = None;
    let mut runtime = None;
    let mut ticks = None;
    let mut stop_file = None;
    let mut list_adapters = false;
    let mut monitor = false;
    let mut goal_command = None;
    let mut plan_command = None;
    let mut task_command = None;
    let mut capability_query = None;
    let mut attempt_query = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--list-adapters" && !list_adapters {
            list_adapters = true;
            continue;
        }
        if argument == "--monitor" && !monitor {
            monitor = true;
            continue;
        }
        let value = args.next().ok_or(USAGE)?;
        if argument == "--config" && config.is_none() {
            config = Some(PathBuf::from(value));
        } else if argument == "--runtime" && runtime.is_none() {
            runtime = Some(PathBuf::from(value));
        } else if argument == "--ticks" && ticks.is_none() {
            let value = value
                .to_str()
                .and_then(|text| text.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .ok_or(USAGE)?;
            ticks = Some(value);
        } else if argument == "--stop-file" && stop_file.is_none() {
            stop_file = Some(PathBuf::from(value));
        } else if argument == "--goal-command" && goal_command.is_none() {
            goal_command = Some(PathBuf::from(value));
        } else if argument == "--plan-command" && plan_command.is_none() {
            plan_command = Some(PathBuf::from(value));
        } else if argument == "--task-command" && task_command.is_none() {
            task_command = Some(PathBuf::from(value));
        } else if argument == "--find-capabilities" && capability_query.is_none() {
            capability_query = Some(PathBuf::from(value));
        } else if argument == "--reasoning-attempts" && attempt_query.is_none() {
            attempt_query = Some(PathBuf::from(value));
        } else {
            return Err(USAGE.into());
        }
    }
    let config = config.ok_or(USAGE)?;
    if (capability_query.is_some() || attempt_query.is_some())
        && (list_adapters
            || (capability_query.is_some() && attempt_query.is_some())
            || runtime.is_some()
            || monitor
            || ticks.is_some()
            || stop_file.is_some()
            || goal_command.is_some()
            || plan_command.is_some()
            || task_command.is_some())
    {
        return Err(USAGE.into());
    }
    let capability_query: Option<arcs::adapters::CapabilityQuery> = capability_query
        .map(|path| -> Result<arcs::adapters::CapabilityQuery, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("capability query exceeds 65536 bytes".into());
            }
            let query: arcs::adapters::CapabilityQuery =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            query.validate().map_err(|e| format!("{e:?}"))?;
            Ok(query)
        })
        .transpose()?;
    let attempt_query: Option<arcs::reasoning::attempts::AttemptQuery> = attempt_query
        .map(
            |path| -> Result<arcs::reasoning::attempts::AttemptQuery, String> {
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .map_err(|e| e.to_string())?
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 65536 {
                    return Err("attempt query exceeds 65536 bytes".into());
                }
                let query: arcs::reasoning::attempts::AttemptQuery =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                query.validate().map_err(|e| format!("{e:?}"))?;
                Ok(query)
            },
        )
        .transpose()?;
    if [
        goal_command.is_some(),
        plan_command.is_some(),
        task_command.is_some(),
    ]
    .into_iter()
    .filter(|value| *value)
    .count()
        > 1
        || (goal_command.is_some() || plan_command.is_some() || task_command.is_some())
            && (runtime.is_none()
                || list_adapters
                || monitor
                || ticks.is_some()
                || stop_file.is_some())
    {
        return Err(USAGE.into());
    }
    let goal_command: Option<GoalCommand> = goal_command
        .map(|path| {
            std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        })
        .transpose()?;
    let plan_command: Option<PlanCommand> = plan_command
        .map(|path| {
            std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        })
        .transpose()?;
    let task_command: Option<TaskControlCommand> = task_command
        .map(|path| -> Result<TaskControlCommand, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("task command exceeds 65536 bytes".into());
            }
            let command: TaskControlCommand =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            command.validate().map_err(|e| e.to_string())?;
            Ok(command)
        })
        .transpose()?;
    if runtime.is_none() && !monitor && (ticks.is_some() || stop_file.is_some()) {
        return Err(USAGE.into());
    }
    if (list_adapters && (runtime.is_some() || monitor)) || (monitor && stop_file.is_none()) {
        return Err(USAGE.into());
    }
    if monitor && stop_file.as_ref().is_some_and(|path| path.exists()) {
        return Err("monitor stop file already exists".into());
    }
    // Validate runtime JSON before opening the Core or starting child processes.
    let runtime = runtime
        .map(RuntimeConfig::load)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut host = Host::open(config).map_err(|e| e.to_string())?;
    if monitor {
        return arcs_host::monitor::run(
            host,
            runtime,
            stop_file.as_deref().unwrap(),
            ticks,
            &mut std::io::stdout().lock(),
        );
    }
    let value = if let Some(query) = attempt_query {
        let attempts = host.reasoning_attempts(&query).map_err(|e| e.to_string())?;
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(attempts)
    } else if let Some(query) = capability_query {
        let matches = host
            .find_capabilities(&query)
            .map_err(|e| format!("{e:?}"))?;
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(matches)
    } else if list_adapters {
        let catalog = host.adapter_catalog();
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(catalog)
    } else if let Some(config) = runtime {
        let mut runtime = TaskRuntime::open(&host, config).map_err(|e| e.to_string())?;
        let report = if let Some(command) = task_command {
            runtime
                .control_task(&mut host, command)
                .map_err(|e| e.to_string())?;
            runtime.report(0).map_err(|e| e.to_string())?
        } else if let Some(PlanCommand::Cancel { sequence, reason }) = plan_command {
            runtime
                .cancel_plan(&mut host, sequence, reason)
                .map_err(|e| e.to_string())?;
            runtime.report(0).map_err(|e| e.to_string())?
        } else if let Some(command) = goal_command {
            match command {
                GoalCommand::ReleaseResources {
                    sequence,
                    reservation_versions,
                    evidence_versions,
                    operator_id,
                    reason,
                } => runtime.reconcile_goal_resources(
                    &host,
                    sequence,
                    &reservation_versions,
                    &evidence_versions,
                    Actor {
                        actor_type: ActorType::Human,
                        id: operator_id,
                    },
                    reason,
                ),
                GoalCommand::Review {
                    sequence,
                    candidate_version_id,
                    decision,
                    operator_id,
                    reason,
                } => runtime.review_goal(
                    &host,
                    sequence,
                    &candidate_version_id,
                    decision,
                    Actor {
                        actor_type: ActorType::Human,
                        id: operator_id,
                    },
                    reason,
                ),
                GoalCommand::Cancel { sequence, reason } => {
                    runtime.cancel_goal(&host, sequence, reason)
                }
            }
            .map_err(|e| e.to_string())?;
            runtime.report(0).map_err(|e| e.to_string())?
        } else {
            runtime
                .run(&mut host, ticks, || {
                    stop_file.as_ref().is_some_and(|path| path.exists())
                })
                .map_err(|e| e.to_string())?
        };
        let reservations = runtime
            .exclusive_resources(&host)
            .map_err(|e| e.to_string())?;
        let selections = runtime
            .reasoner_selections(&host)
            .map_err(|e| e.to_string())?;
        let recoveries = runtime
            .reasoning_recoveries(&host)
            .map_err(|e| e.to_string())?;
        let reconnects = host.adapter_reconnections().map_err(|e| e.to_string())?;
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(report).map(|mut value| {
            if !reconnects.is_empty() {
                value["adapter_reconnections"] =
                    serde_json::to_value(reconnects).expect("reconnect report serialization");
            }
            if !recoveries.is_empty() {
                value["reasoning_recoveries"] =
                    serde_json::to_value(recoveries).expect("recovery report serialization");
            }
            if !selections.is_empty() {
                value["reasoner_selections"] =
                    serde_json::to_value(selections).expect("selection report serialization");
            }
            if !reservations.is_empty() {
                value["resource_reservations"] =
                    serde_json::to_value(reservations).expect("reservation serialization");
            }
            value
        })
    } else {
        serde_json::to_value(host.run().map_err(|e| e.to_string())?)
    }
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("arcs-host: {error}");
        std::process::exit(1);
    }
}

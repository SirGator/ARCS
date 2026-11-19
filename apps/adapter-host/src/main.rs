use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use arcs_host::Host;
use arcs_host::memory::MemoryCommand;
use arcs_host::monitor::MonitorControl;
use arcs_host::operator_command::{CommandReceipt, GoalCommand, OperatorCommand, PlanCommand};
use arcs_host::task_runtime::scheduler::SeriesControlCommand;
use arcs_host::task_runtime::suspension::TaskControlCommand;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};
use serde::Deserialize;

const USAGE: &str = "usage: arcs-host --config <host.json> [--list-adapters | --find-capabilities <query.json> | --reasoning-attempts <query.json> | --memory-command <command.json> | [--monitor --stop-file <path> [--control-stdin]] [--runtime <runtime.json>] [--ticks <count>] [--stop-file <path>] | --runtime <runtime.json> [--goal-command <command.json> | --plan-command <command.json> | --task-command <command.json> | --series-command <command.json>]]";

fn read_control_command<T: serde::de::DeserializeOwned>(
    path: PathBuf,
    label: &str,
) -> Result<T, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65536 {
        return Err(format!("{label} command exceeds 65536 bytes"));
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
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
    let mut control_stdin = false;
    let mut goal_command = None;
    let mut plan_command = None;
    let mut task_command = None;
    let mut series_command = None;
    let mut capability_query = None;
    let mut attempt_query = None;
    let mut memory_command = None;
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
        if argument == "--control-stdin" && !control_stdin {
            control_stdin = true;
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
        } else if argument == "--series-command" && series_command.is_none() {
            series_command = Some(PathBuf::from(value));
        } else if argument == "--find-capabilities" && capability_query.is_none() {
            capability_query = Some(PathBuf::from(value));
        } else if argument == "--reasoning-attempts" && attempt_query.is_none() {
            attempt_query = Some(PathBuf::from(value));
        } else if argument == "--memory-command" && memory_command.is_none() {
            memory_command = Some(PathBuf::from(value));
        } else {
            return Err(USAGE.into());
        }
    }
    let config = config.ok_or(USAGE)?;
    if control_stdin && (!monitor || runtime.is_none()) {
        return Err(USAGE.into());
    }
    let runtime_commands = [&goal_command, &plan_command, &task_command, &series_command]
        .into_iter()
        .filter(|command| command.is_some())
        .count();
    if memory_command.is_some()
        && (capability_query.is_some()
            || attempt_query.is_some()
            || list_adapters
            || runtime.is_some()
            || monitor
            || ticks.is_some()
            || stop_file.is_some()
            || runtime_commands > 0)
    {
        return Err(USAGE.into());
    }
    let memory_command: Option<MemoryCommand> = memory_command
        .map(|path| -> Result<MemoryCommand, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("memory command exceeds 65536 bytes".into());
            }
            let command: MemoryCommand =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            command.validate().map_err(|e| e.to_string())?;
            Ok(command)
        })
        .transpose()?;
    if (capability_query.is_some() || attempt_query.is_some())
        && (list_adapters
            || (capability_query.is_some() && attempt_query.is_some())
            || runtime.is_some()
            || monitor
            || ticks.is_some()
            || stop_file.is_some()
            || runtime_commands > 0)
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
    if runtime_commands > 1
        || runtime_commands > 0
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
            let command: TaskControlCommand = read_control_command(path, "task")?;
            command.validate().map_err(|e| e.to_string())?;
            Ok(command)
        })
        .transpose()?;
    let series_command: Option<SeriesControlCommand> = series_command
        .map(|path| -> Result<SeriesControlCommand, String> {
            let command: SeriesControlCommand = read_control_command(path, "series")?;
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
        let control = control_stdin
            .then(MonitorControl::from_stdin)
            .transpose()
            .map_err(|error| error.to_string())?;
        return arcs_host::monitor::run_with_control(
            host,
            runtime,
            stop_file.as_deref().unwrap(),
            ticks,
            control.as_ref(),
            &mut std::io::stdout().lock(),
        );
    }
    let value = if let Some(command) = memory_command {
        let result = match command {
            MemoryCommand::Assess { assessment } => {
                serde_json::to_value(host.assess_memory(&assessment).map_err(|e| e.to_string())?)
            }
            MemoryCommand::Inspect { target_version_ids } => serde_json::to_value(
                host.inspect_memory(&target_version_ids)
                    .map_err(|e| e.to_string())?,
            ),
        };
        host.shutdown().map_err(|e| e.to_string())?;
        result
    } else if let Some(query) = attempt_query {
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
        let command = task_command
            .map(OperatorCommand::Task)
            .or_else(|| series_command.map(OperatorCommand::Series))
            .or_else(|| plan_command.map(OperatorCommand::Plan))
            .or_else(|| goal_command.map(OperatorCommand::Goal));
        let (report, receipt) = if let Some(command) = command {
            let receipt = command
                .apply(&mut runtime, &mut host)
                .map_err(|e| e.to_string())?;
            (runtime.report(0).map_err(|e| e.to_string())?, receipt)
        } else {
            let report = runtime
                .run(&mut host, ticks, || {
                    stop_file.as_ref().is_some_and(|path| path.exists())
                })
                .map_err(|e| e.to_string())?;
            (report, None)
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
            match receipt {
                Some(CommandReceipt::Task(receipt)) => {
                    value["task_control"] =
                        serde_json::to_value(receipt).expect("task control receipt serialization")
                }
                Some(CommandReceipt::Series(receipt)) => {
                    value["series_control"] =
                        serde_json::to_value(receipt).expect("series control receipt serialization")
                }
                Some(CommandReceipt::GoalReview(review)) => {
                    value["goal_review"] =
                        serde_json::to_value(review).expect("goal review serialization")
                }
                Some(CommandReceipt::ResourceEvidenceReview(review)) => {
                    value["resource_evidence_review"] = serde_json::to_value(review)
                        .expect("resource evidence review serialization")
                }
                _ => {}
            }
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

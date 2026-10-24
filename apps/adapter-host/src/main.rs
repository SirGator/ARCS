use std::ffi::OsString;
use std::path::PathBuf;

use arcs_host::Host;
use arcs_host::task_runtime::{RuntimeConfig, TaskRuntime};

const USAGE: &str = "usage: arcs-host --config <host.json> [--list-adapters | [--monitor --stop-file <path>] [--runtime <runtime.json>] [--ticks <count>] [--stop-file <path>]]";
fn run() -> Result<(), String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let mut config = None;
    let mut runtime = None;
    let mut ticks = None;
    let mut stop_file = None;
    let mut list_adapters = false;
    let mut monitor = false;
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
        } else {
            return Err(USAGE.into());
        }
    }
    let config = config.ok_or(USAGE)?;
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
    let value = if list_adapters {
        let catalog = host.adapter_catalog();
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(catalog)
    } else if let Some(config) = runtime {
        let mut runtime = TaskRuntime::open(&host, config).map_err(|e| e.to_string())?;
        let report = runtime
            .run(&mut host, ticks, || {
                stop_file.as_ref().is_some_and(|path| path.exists())
            })
            .map_err(|e| e.to_string())?;
        host.shutdown().map_err(|e| e.to_string())?;
        serde_json::to_value(report)
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

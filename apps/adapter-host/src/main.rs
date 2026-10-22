use std::ffi::OsString;
use std::path::PathBuf;

use arcs_host::Host;

fn run() -> Result<(), String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("usage: arcs-host --config <host.json>");
        return Ok(());
    }
    if args.len() != 2 || args[0] != "--config" {
        return Err("usage: arcs-host --config <host.json>".into());
    }
    let host = Host::open(PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
    let report = host.run().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("arcs-host: {error}");
        std::process::exit(1);
    }
}

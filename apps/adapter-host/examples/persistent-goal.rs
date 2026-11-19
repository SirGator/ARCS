mod support {
    pub mod persistent_goal;
}
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::persistent_goal::{Options, qualify};

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 || args[0] != "--approve-simulated-actions" {
        eprintln!("usage: persistent-goal --approve-simulated-actions <host.json> <runtime.json>");
        std::process::exit(2);
    }
    let executable = std::env::current_exe().expect("example executable");
    let profile = executable
        .parent()
        .and_then(|p| p.parent())
        .expect("Cargo example directory");
    let options = Options {
        host: PathBuf::from(&args[1]),
        runtime: PathBuf::from(&args[2]),
        simulator_program: profile.join(format!(
            "arcs-simulated-fan{}",
            std::env::consts::EXE_SUFFIX
        )),
        start_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("UTC time")
            .as_millis() as u64,
        phase_timeout: Duration::from_secs(750),
    };
    let evidence = qualify(&options);
    println!(
        "{}",
        serde_json::to_string_pretty(&evidence).expect("evidence JSON")
    );
    if !evidence.passed {
        std::process::exit(1);
    }
}

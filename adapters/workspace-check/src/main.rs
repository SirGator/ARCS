mod runner;

use std::io::{self, BufReader};
use std::path::Path;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [profile, journal] = args.as_slice() else {
        return Err("usage: arcs-workspace-check <profile.json> <journal.sqlite>".into());
    };
    let mut runner = runner::Runner::open(Path::new(profile), Path::new(journal))?;
    arcs_adapter_sdk::serve(
        &mut runner,
        &mut BufReader::new(io::stdin().lock()),
        &mut io::stdout().lock(),
    )
    .map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("workspace check: {error}");
        std::process::exit(1);
    }
}

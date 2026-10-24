fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: arcs-json-report <profile.json>".into());
    }
    let profile =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let mut adapter = arcs_json_report::JsonReport::new(profile)?;
    arcs_adapter_sdk::serve(
        &mut adapter,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )
    .map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("JSON report: {error}");
        std::process::exit(1);
    }
}

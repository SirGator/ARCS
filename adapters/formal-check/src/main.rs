fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut adapter_id = arcs_formal_check::ADAPTER.to_owned();
    let mut binary: Option<std::path::PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--adapter-id") => {
                let value = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or("the adapter ID must be UTF-8")?;
                adapter_id = value.to_owned();
                index += 2;
            }
            Some("--lean-binary") => {
                let value = args
                    .get(index + 1)
                    .ok_or("the Lean binary needs a path")?
                    .to_owned();
                binary = Some(std::path::PathBuf::from(value));
                index += 2;
            }
            _ => {
                return Err(
                    "usage: arcs-formal-check [--adapter-id <id>] [--lean-binary <path>]".into(),
                );
            }
        }
    }
    let checker = match binary {
        Some(binary) => arcs_formal_check::FormalChecker::with_binary(binary),
        None => arcs_formal_check::FormalChecker::new(),
    };
    let mut checker = arcs_formal_check::FormalCheckerInstance::with_binary(
        arcs_adapter_protocol::AdapterId(adapter_id),
        checker,
    )?;
    arcs_adapter_sdk::serve(
        &mut checker,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("formal check: {error}");
        std::process::exit(1);
    }
}

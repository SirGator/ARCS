fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: arcs-file-output <profile.json> <journal.sqlite>".into());
    }
    let mut adapter = arcs_file_output::FileOutput::open(
        std::path::Path::new(&args[0]),
        std::path::Path::new(&args[1]),
    )?;
    arcs_adapter_sdk::serve(
        &mut adapter,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )
    .map_err(|error| error.to_string())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("file output: {error}");
        std::process::exit(1);
    }
}

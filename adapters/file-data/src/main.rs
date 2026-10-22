use std::io::{self, BufReader};
use std::path::Path;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: arcs-file-data <profile.json> <adapter-state.sqlite>".into());
    }
    let mut adapter = arcs_file_data::FileData::open(Path::new(&args[0]), Path::new(&args[1]))?;
    arcs_adapter_sdk::serve(
        &mut adapter,
        &mut BufReader::new(io::stdin().lock()),
        &mut io::stdout().lock(),
    )
    .map_err(|error| error.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("file data: {error}");
        std::process::exit(1);
    }
}

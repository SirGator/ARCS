fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: arcs-simulated-drone <profile.json> <state.sqlite>".into());
    }
    let profile = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut adapter =
        arcs_simulated_drone::SimulatedDrone::open(profile, std::path::Path::new(&args[1]))?;
    arcs_adapter_sdk::serve(
        &mut adapter,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("simulated drone: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: arcs-drone-demo <host.json> <flight.json>".into());
    }
    let config = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let mut host = arcs_host::Host::open(std::path::Path::new(&args[0]))?;
    let report = arcs_drone_demo::run(&mut host, &config)?;
    host.shutdown()?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("drone demo: {error}");
        std::process::exit(1);
    }
}

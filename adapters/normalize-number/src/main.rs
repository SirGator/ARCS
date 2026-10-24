fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let adapter_id = match args.as_slice() {
        [] => arcs_normalize_number::ADAPTER,
        [flag, id] if flag == "--adapter-id" => id.to_str().ok_or("adapter ID must be UTF-8")?,
        _ => return Err("usage: arcs-normalize-number [--adapter-id <id>]".into()),
    };
    let mut normalizer = arcs_normalize_number::NormalizerInstance::new(
        arcs_adapter_protocol::AdapterId(adapter_id.into()),
    )?;
    arcs_adapter_sdk::serve(
        &mut normalizer,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("normalize number: {error}");
        std::process::exit(1);
    }
}

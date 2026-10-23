fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("usage: arcs-normalize-number");
        std::process::exit(1);
    }
    let result = arcs_adapter_sdk::serve(
        &mut arcs_normalize_number::Normalizer,
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    );
    if let Err(error) = result {
        eprintln!("normalize number: {error}");
        std::process::exit(1);
    }
}

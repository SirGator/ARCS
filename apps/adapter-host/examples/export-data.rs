//! Beispiel für eine Anwendung des allgemeinen Hosts ohne Dateiquellen-Abhängigkeit.
use std::io::Write;
use std::path::{Path, PathBuf};

use arcs::adapters::{CapabilityRef, data::DataReference};
use arcs_host::Host;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: export-data <host.json> <adapter-id> <capability-id> <reference-id> <output-file>".into());
    }
    let mut host = Host::open(&args[0])?;
    host.poll_round()?;
    let capability = CapabilityRef::new(&args[1], &args[2]);
    // Die Referenz kommt aus einer persistierten Observation des gewählten
    // Adapters. Die Capability wird separat vom Betreiber angegeben.
    let mut reference = None;
    for estimate in host.report()?.estimates {
        if estimate.schema_id.0 != "arcs.data.reference.v1" {
            continue;
        }
        let artifact = host
            .store()
            .get(&estimate.evidence_version)
            .map_err(arcs_host::HostError::Store)?
            .ok_or("missing evidence artifact")?;
        if artifact.created_by.id != args[1]
            || !artifact.tags.contains(&format!("capability:{}", args[2]))
        {
            continue;
        }
        let candidate: DataReference = serde_json::from_value(estimate.value)?;
        if candidate.id == args[3] {
            reference = Some(candidate);
            break;
        }
    }
    let reference = reference.ok_or("no matching persisted data reference")?;
    let output = Path::new(&args[4]);
    let partial = PathBuf::from(format!(
        "{}.partial-{}",
        output.display(),
        std::process::id()
    ));
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| -> Result<u64, Box<dyn std::error::Error>> {
        let count = host.copy_data(&capability, &reference, &mut writer)?;
        writer.flush()?;
        writer.sync_all()?;
        drop(writer);
        // hard_link publiziert erst nach vollständigem Abruf und überschreibt
        // keine bereits vorhandene Zieldatei.
        std::fs::hard_link(&partial, output)?;
        Ok(count)
    })();
    let _ = std::fs::remove_file(&partial);
    let count = result?;
    host.shutdown()?;
    println!("exported {count} bytes to {}", output.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("export data: {error}");
        std::process::exit(1);
    }
}

//! Allgemeine Ausgabe geprüfter Berichte, ohne Core-Abhängigkeit.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
    OutputInvocation, OutputResponse, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::json;

pub const CAPABILITY: &str = "report.write";
pub const REPORT_SCHEMA: &str = "arcs.report.v1";
pub const RECEIPT_SCHEMA: &str = "arcs.delivery_receipt.file.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    title: String,
    body: String,
}

fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}
fn rejected(message: &str) -> AdapterError {
    AdapterError::rejected(message)
}

pub struct FileOutput {
    manifest: AdapterManifest,
    directory: PathBuf,
    journal: Connection,
}

impl FileOutput {
    pub fn open(profile_path: &Path, journal_path: &Path) -> Result<Self, String> {
        let profile_path = profile_path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let profile: Profile = serde_json::from_slice(
            &std::fs::read(&profile_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if profile.directory.as_os_str().is_empty() {
            return Err("empty output directory".into());
        }
        let manifest = AdapterManifest {
            protocol_version: 1,
            adapter_id: profile.adapter_id,
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId(CAPABILITY.into()),
                contract: CapabilityContract::Output {
                    accepts: vec![SchemaId(REPORT_SCHEMA.into())],
                    emits: vec![SchemaId(RECEIPT_SCHEMA.into())],
                    idempotent: true,
                },
                required_permissions: vec!["file.write".into()],
            }],
        };
        manifest.validate().map_err(|error| format!("{error:?}"))?;
        let directory = profile_path.parent().unwrap().join(profile.directory);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let directory = directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let mut journal = Connection::open(journal_path).map_err(|error| error.to_string())?;
        journal.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS sink_identity (singleton INTEGER PRIMARY KEY CHECK(singleton=1),adapter_id TEXT NOT NULL,directory TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS deliveries (invocation_id TEXT PRIMARY KEY,input_json TEXT NOT NULL,response_json TEXT NOT NULL,filename TEXT NOT NULL UNIQUE);").map_err(|error|error.to_string())?;
        let transaction = journal
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let destination = directory.to_str().ok_or("output directory must be UTF-8")?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO sink_identity VALUES (1,?1,?2)",
                params![manifest.adapter_id.0, destination],
            )
            .map_err(|error| error.to_string())?;
        let identity: (String, String) = transaction
            .query_row(
                "SELECT adapter_id,directory FROM sink_identity WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        if identity != (manifest.adapter_id.0.clone(), destination.into()) {
            return Err("journal belongs to another output installation or directory".into());
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(Self {
            manifest,
            directory,
            journal,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Adapter for FileOutput {
    fn manifest(&self) -> AdapterManifest {
        self.manifest.clone()
    }

    fn output(&mut self, invocation: OutputInvocation) -> Result<OutputResponse, AdapterError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id
            || invocation.capability.capability_id.0 != CAPABILITY
            || invocation.artifact_schema_id.0 != REPORT_SCHEMA
            || invocation.result_schema_id.0 != RECEIPT_SCHEMA
            || invocation.request_version_id.0.trim().is_empty()
            || invocation.approval_version_id.0.trim().is_empty()
            || invocation.artifact_version_id.0.trim().is_empty()
            || invocation.invocation_id.is_empty()
            || invocation.invocation_id.len() > 96
            || !invocation
                .invocation_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
        {
            return Err(rejected("output contract mismatch"));
        }
        let report: Report = serde_json::from_value(invocation.payload.clone())
            .map_err(|error| AdapterError::rejected(error.to_string()))?;
        if report.title.trim().is_empty()
            || report.title.len() > 256
            || report.title.chars().any(char::is_control)
            || report.body.len() > 60 * 1024
        {
            return Err(rejected("invalid report"));
        }
        let input = serde_json::to_string(&invocation).map_err(unavailable)?;
        let mut bytes = serde_json::to_vec_pretty(&invocation).map_err(unavailable)?;
        bytes.push(b'\n');
        let encoded: String = invocation
            .invocation_id
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let filename = format!("output-{encoded}.json");
        let target = self.directory.join(&filename);
        let response = OutputResponse {
            invocation_id: invocation.invocation_id.clone(),
            external_reference: format!("file-output:{filename}"),
            result_payload: json!({"artifact_version":invocation.artifact_version_id,"file_name":filename,"bytes_written":bytes.len(),"status":"delivered"}),
        };
        if serde_json::to_vec(&response.result_payload)
            .map_err(unavailable)?
            .len()
            > invocation.max_result_bytes
        {
            return Err(rejected("output receipt exceeds caller limit"));
        }
        let transaction = self
            .journal
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let previous: Option<(String, String)> = transaction
            .query_row(
                "SELECT input_json,response_json FROM deliveries WHERE invocation_id=?1",
                [&invocation.invocation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(unavailable)?;
        if let Some((previous, response)) = previous {
            if previous != input {
                return Err(rejected("output invocation identity conflict"));
            }
            require_content(&target, &bytes)?;
            return serde_json::from_str(&response).map_err(unavailable);
        }
        publish(&self.directory, &target, &bytes)?;
        // Bei einem Abbruch vor diesem Commit belegt die vollständig
        // veröffentlichte Datei die genaue Invocation für den nächsten Versuch.
        transaction
            .execute(
                "INSERT INTO deliveries VALUES (?1,?2,?3,?4)",
                params![
                    invocation.invocation_id,
                    input,
                    serde_json::to_string(&response).map_err(unavailable)?,
                    filename
                ],
            )
            .map_err(unavailable)?;
        transaction.commit().map_err(unavailable)?;
        Ok(response)
    }
}

fn require_content(path: &Path, expected: &[u8]) -> Result<(), AdapterError> {
    let metadata = std::fs::symlink_metadata(path).map_err(unavailable)?;
    if !metadata.file_type().is_file() || metadata.len() != expected.len() as u64 {
        return Err(rejected("output file conflicts with expected delivery"));
    }
    let mut actual = Vec::new();
    std::fs::File::open(path)
        .map_err(unavailable)?
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut actual)
        .map_err(unavailable)?;
    if actual != expected {
        return Err(rejected("output file conflicts with expected delivery"));
    }
    Ok(())
}

struct PartialFile(PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn publish(directory: &Path, target: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    if target.try_exists().map_err(unavailable)? {
        require_content(target, bytes)?;
        sync_directory(directory)?;
        return Ok(());
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let partial = directory.join(format!(
        ".partial-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(unavailable)?;
    let cleanup = PartialFile(partial);
    writer.write_all(bytes).map_err(unavailable)?;
    writer.sync_all().map_err(unavailable)?;
    drop(writer);
    match std::fs::hard_link(&cleanup.0, target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_content(target, bytes)?
        }
        Err(error) => return Err(unavailable(error)),
    }
    sync_directory(directory)?;
    Ok(())
}

fn sync_directory(directory: &Path) -> Result<(), AdapterError> {
    #[cfg(unix)]
    std::fs::File::open(directory)
        .map_err(unavailable)?
        .sync_all()
        .map_err(unavailable)?;
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

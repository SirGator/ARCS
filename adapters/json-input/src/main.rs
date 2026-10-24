use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId,
    InputMessage, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingInput};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    id: String,
    subject: String,
    reference: String,
    payload: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    capability_id: CapabilityId,
    schema_id: SchemaId,
    events: Vec<Event>,
}
struct Source {
    manifest: AdapterManifest,
    database: Connection,
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}
impl Adapter for Source {
    fn manifest(&self) -> AdapterManifest {
        self.manifest.clone()
    }
    fn poll_input(&mut self) -> Result<Option<PendingInput>, AdapterError> {
        let event: Option<(String, String)> = self
            .database
            .query_row(
                "SELECT id,message FROM events WHERE ack=0 ORDER BY sequence LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(unavailable)?;
        event
            .map(|(message_id, json)| {
                Ok(PendingInput {
                    message_id,
                    input: serde_json::from_str(&json).map_err(unavailable)?,
                })
            })
            .transpose()
    }
    fn acknowledge_input(&mut self, id: &str) -> Result<(), AdapterError> {
        if self
            .database
            .execute("UPDATE events SET ack=1 WHERE id=?1", [id])
            .map_err(unavailable)?
            == 0
        {
            return Err(AdapterError::rejected("unknown input ACK"));
        }
        Ok(())
    }
}
fn validate_event(event: &Event) -> Result<(), Box<dyn std::error::Error>> {
    if event.id.trim().is_empty()
        || event.id.len() > 128
        || event.subject.trim().is_empty()
        || event.reference.trim().is_empty()
    {
        return Err("invalid input event identity".into());
    }
    Ok(())
}
fn enqueue(
    transaction: &Transaction<'_>,
    capability: &CapabilityId,
    event: &Event,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_event(event)?;
    let input = InputMessage {
        capability_id: capability.clone(),
        external_subject: Some(event.subject.clone()),
        external_reference: event.reference.clone(),
        payload: event.payload.clone(),
    };
    let message = serde_json::to_string(&input)?;
    if message.len() > 60 * 1024 {
        return Err("input message exceeds 60 KiB".into());
    }
    transaction.execute(
        "INSERT OR IGNORE INTO events(id,message) VALUES(?1,?2)",
        params![event.id, message],
    )?;
    let previous: String = transaction.query_row(
        "SELECT message FROM events WHERE id=?1",
        [&event.id],
        |row| row.get(0),
    )?;
    if previous != message {
        return Err("input event identity conflict".into());
    }
    Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (profile_path,journal_path,event_path)=match args.as_slice() {
        [profile,journal] => (profile,journal,None),
        [flag,profile,journal,event] if flag=="--enqueue" => (profile,journal,Some(event)),
        _ => return Err("usage: arcs-json-input <profile.json> <journal.sqlite> OR arcs-json-input --enqueue <profile.json> <journal.sqlite> <event.json>".into()),
    };
    let event: Option<Event> = event_path
        .map(|path| -> Result<Event, Box<dyn std::error::Error>> {
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?;
    if let Some(event) = &event {
        validate_event(event)?;
    }
    let profile: Profile = serde_json::from_slice(&std::fs::read(profile_path)?)?;
    let manifest = AdapterManifest {
        protocol_version: 1,
        adapter_id: profile.adapter_id.clone(),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![CapabilityDescriptor {
            id: profile.capability_id.clone(),
            contract: CapabilityContract::Input {
                emits: vec![profile.schema_id.clone()],
            },
            required_permissions: vec!["input.read".into()],
        }],
    };
    manifest.validate().map_err(|error| format!("{error:?}"))?;
    let mut ids = std::collections::HashSet::new();
    for event in &profile.events {
        validate_event(event)?;
        if !ids.insert(&event.id) {
            return Err("invalid or duplicate input event identity".into());
        }
    }
    let identity = serde_json::to_string(&profile)?;
    let mut database = Connection::open(journal_path)?;
    database.busy_timeout(std::time::Duration::from_secs(2))?;
    database.execute_batch("PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS installation (singleton INTEGER PRIMARY KEY CHECK(singleton=1),profile TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS events (sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,message TEXT NOT NULL,ack INTEGER NOT NULL DEFAULT 0);")?;
    let transaction = database.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO installation VALUES(1,?1)",
        [&identity],
    )?;
    let previous: String =
        transaction.query_row("SELECT profile FROM installation", [], |row| row.get(0))?;
    if previous != identity {
        return Err("input journal profile identity conflict".into());
    }
    if inserted == 1 {
        for event in &profile.events {
            enqueue(&transaction, &profile.capability_id, event)?;
        }
    }
    if let Some(event) = &event {
        enqueue(&transaction, &profile.capability_id, event)?;
    }
    transaction.commit()?;
    if let Some(event) = event {
        println!("{}", serde_json::json!({"queued":event.id}));
        return Ok(());
    }
    arcs_adapter_sdk::serve(
        &mut Source { manifest, database },
        &mut std::io::BufReader::new(std::io::stdin().lock()),
        &mut std::io::stdout().lock(),
    )?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("json input: {error}");
        std::process::exit(1);
    }
}

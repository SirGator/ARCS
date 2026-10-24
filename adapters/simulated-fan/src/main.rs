//! Persistenter Simulator; keine Hardware und kein Gerätewissen im Core.

use arcs_simulated_fan as fan;

use std::io::{self, BufReader};

use arcs_adapter_protocol::ObservationMessage;
use arcs_adapter_protocol::SchemaId;
use arcs_adapter_protocol::stdio::AdapterMessage;
use arcs_adapter_protocol::{ActInvocation, ActResponse, CapabilityId, CapabilityRef};
use arcs_adapter_protocol::{RequestInvocation, RequestResponse};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingObservation, serve};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;
use serde_json::{Value, json};

struct Simulator {
    adapter_id: arcs_adapter_protocol::AdapterId,
    profile: fan::Profile,
    database: Connection,
}

fn error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn state(database: &Connection) -> Result<fan::FanState, String> {
    database
        .query_row(
            "SELECT device_id, enabled, actuation_count FROM fan_state WHERE singleton = 1",
            [],
            |row| {
                Ok(fan::FanState {
                    device_id: row.get(0)?,
                    enabled: row.get(1)?,
                    actuation_count: row.get::<_, i64>(2)? as u64,
                })
            },
        )
        .map_err(error)
}

fn enqueue(
    database: &Connection,
    id: &str,
    capability: &str,
    subject: &str,
    payload: Value,
) -> Result<(), String> {
    let observation = ObservationMessage {
        capability_id: CapabilityId(capability.into()),
        external_subject: Some(subject.into()),
        external_reference: format!("simulated:{id}"),
        payload,
    };
    database
        .execute(
            "INSERT INTO observations (message_id, message_json) VALUES (?1, ?2)",
            params![id, serde_json::to_string(&observation).map_err(error)?],
        )
        .map_err(error)?;
    Ok(())
}

fn cached(database: &Connection, id: &str, input: &str) -> Result<Option<AdapterMessage>, String> {
    let existing: Option<(String, String)> = database
        .query_row(
            "SELECT input_json, response_json FROM invocations WHERE invocation_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(error)?;
    existing
        .map(|(previous, response)| {
            if previous != input {
                return Err("invocation identity conflict".into());
            }
            serde_json::from_str(&response).map_err(error)
        })
        .transpose()
}

fn remember(
    transaction: &Transaction<'_>,
    id: &str,
    input: &str,
    response: &AdapterMessage,
) -> Result<(), String> {
    transaction.execute("INSERT INTO invocations (invocation_id, input_json, response_json) VALUES (?1, ?2, ?3)",
        params![id, input, serde_json::to_string(response).map_err(error)?]).map_err(error)?;
    Ok(())
}

impl Simulator {
    fn open(
        profile: fan::Profile,
        path: &std::path::Path,
        adapter_id: arcs_adapter_protocol::AdapterId,
    ) -> Result<Self, String> {
        fan::manifest_with_id(adapter_id.clone())
            .validate()
            .map_err(error)?;
        let mut database = Connection::open(path).map_err(error)?;
        database.execute_batch("
            CREATE TABLE IF NOT EXISTS fan_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1), device_id TEXT NOT NULL,
                enabled INTEGER NOT NULL, actuation_count INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS invocations (
                invocation_id TEXT PRIMARY KEY, input_json TEXT NOT NULL, response_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS observations (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT, message_id TEXT NOT NULL UNIQUE,
                message_json TEXT NOT NULL, acknowledged INTEGER NOT NULL DEFAULT 0
            );
        ").map_err(error)?;
        let transaction = database.transaction().map_err(error)?;
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS adapter_installation (singleton INTEGER PRIMARY KEY CHECK(singleton=1),adapter_id TEXT NOT NULL);").map_err(error)?;
        let bound: Option<String> = transaction
            .query_row(
                "SELECT adapter_id FROM adapter_installation WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        let legacy: bool = transaction
            .query_row("SELECT EXISTS(SELECT 1 FROM fan_state)", [], |row| {
                row.get(0)
            })
            .map_err(error)?;
        if bound.as_ref().is_some_and(|id| id != &adapter_id.0)
            || (bound.is_none() && legacy && adapter_id.0 != fan::ADAPTER)
        {
            return Err("adapter database belongs to another installation".into());
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO adapter_installation VALUES(1,?1)",
                [&adapter_id.0],
            )
            .map_err(error)?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO fan_state VALUES (1, ?1, 0, 0)",
                [&profile.fan_device],
            )
            .map_err(error)?;
        let current = state(&transaction)?;
        if current.device_id != profile.fan_device {
            return Err("adapter database belongs to another fan".into());
        }
        if inserted == 1 {
            enqueue(
                &transaction,
                "fan:initial",
                "fan.observe",
                &profile.fan_device,
                serde_json::to_value(current).map_err(error)?,
            )?;
        }
        transaction.commit().map_err(error)?;
        Ok(Self {
            adapter_id,
            profile,
            database,
        })
    }

    fn fetch(&mut self, invocation: RequestInvocation) -> Result<AdapterMessage, String> {
        let (input_schema, output_schema, device, observation_capability) =
            match invocation.capability.capability_id.0.as_str() {
                "temperature.read" => (
                    fan::TEMPERATURE_REQUEST,
                    fan::TEMPERATURE,
                    &self.profile.temperature_device,
                    "temperature.observe",
                ),
                "fan.read" => (
                    fan::FAN_REQUEST,
                    fan::FAN_STATE,
                    &self.profile.fan_device,
                    "fan.observe",
                ),
                _ => return Err("unknown request capability".into()),
            };
        if invocation.capability.adapter_id != self.adapter_id
            || invocation.request_schema_id.0 != input_schema
            || invocation.response_schema_id.0 != output_schema
            || invocation.request_payload["device_id"].as_str() != Some(device)
            || invocation.invocation_id.trim().is_empty()
        {
            return Err("request contract mismatch".into());
        }
        let query: fan::DeviceQuery =
            serde_json::from_value(invocation.request_payload.clone()).map_err(error)?;
        if query.device_id.trim().is_empty() {
            return Err("empty device ID".into());
        }
        let input = serde_json::to_string(&("request", &invocation)).map_err(error)?;
        let transaction = self.database.transaction().map_err(error)?;
        if let Some(response) = cached(&transaction, &invocation.invocation_id, &input)? {
            return Ok(response);
        }
        let payload = if observation_capability == "temperature.observe" {
            json!({"value": self.profile.temperature(), "unit": self.profile.unit})
        } else {
            serde_json::to_value(state(&transaction)?).map_err(error)?
        };
        if serde_json::to_vec(&payload).map_err(error)?.len() > invocation.max_response_bytes {
            return Err("request response limit exceeded".into());
        }

        let event_id = format!("{}:observed", invocation.invocation_id);
        enqueue(
            &transaction,
            &event_id,
            observation_capability,
            device,
            payload.clone(),
        )?;
        let response = AdapterMessage::RequestResult {
            response: RequestResponse {
                invocation_id: invocation.invocation_id.clone(),
                external_reference: format!("simulated:{event_id}"),
                payload,
            },
        };
        remember(&transaction, &invocation.invocation_id, &input, &response)?;
        transaction.commit().map_err(error)?;
        Ok(response)
    }

    fn execute(&mut self, invocation: ActInvocation) -> Result<AdapterMessage, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Materialized {
            target_version: String,
            approval_version: String,
            capability: CapabilityRef,
            input_schema_id: SchemaId,
            payload: fan::FanCommand,
        }
        if invocation.capability != CapabilityRef::new(&self.adapter_id.0, "fan.set")
            || invocation.action_schema_id.0 != "arcs.action.v1"
            || invocation.result_schema_id.0 != fan::RESULT
            || invocation.invocation_id.trim().is_empty()
            || invocation.action_version_id.0.trim().is_empty()
        {
            return Err("action contract mismatch".into());
        }
        let materialized: Materialized =
            serde_json::from_value(invocation.payload.clone()).map_err(error)?;
        if materialized.capability != invocation.capability
            || materialized.input_schema_id.0 != fan::CANDIDATE
            || materialized.target_version.trim().is_empty()
            || materialized.approval_version.trim().is_empty()
            || materialized.payload.device_id != self.profile.fan_device
        {
            return Err("materialized action mismatch".into());
        }

        let input = serde_json::to_string(&("execute", &invocation)).map_err(error)?;
        let transaction = self.database.transaction().map_err(error)?;
        if let Some(response) = cached(&transaction, &invocation.invocation_id, &input)? {
            return Ok(response);
        }
        // Simulierter Gerätezustand, Journal und Observation werden atomar
        // geschrieben. Bei echter Hardware braucht der Treiber deren eigene
        // Idempotenz-/Recovery-Garantien.
        transaction.execute("UPDATE fan_state SET actuation_count = actuation_count + CASE WHEN enabled != ?1 THEN 1 ELSE 0 END, enabled = ?1 WHERE singleton = 1",
            [materialized.payload.enabled]).map_err(error)?;
        let current = state(&transaction)?;
        let payload = serde_json::to_value(fan::ExecutionResult {
            action_version: invocation.action_version_id.clone(),
            result: current.clone(),
        })
        .map_err(error)?;
        let event_id = format!("{}:observed", invocation.invocation_id);
        enqueue(
            &transaction,
            &event_id,
            "fan.observe",
            &self.profile.fan_device,
            serde_json::to_value(&current).map_err(error)?,
        )?;
        let response = AdapterMessage::ExecutionResult {
            response: ActResponse {
                invocation_id: invocation.invocation_id.clone(),
                external_reference: format!("simulated:{event_id}"),
                result_payload: payload,
            },
        };
        remember(&transaction, &invocation.invocation_id, &input, &response)?;
        transaction.commit().map_err(error)?;
        Ok(response)
    }

    fn poll(&self) -> Result<AdapterMessage, String> {
        let pending: Option<(String, String)> = self.database.query_row(
            "SELECT message_id, message_json FROM observations WHERE acknowledged = 0 ORDER BY sequence LIMIT 1", [],
            |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(error)?;
        match pending {
            Some((message_id, json)) => Ok(AdapterMessage::Observation {
                message_id,
                observation: serde_json::from_str(&json).map_err(error)?,
            }),
            None => Ok(AdapterMessage::Idle),
        }
    }

    fn ack(&self, message_id: String) -> Result<AdapterMessage, String> {
        let changed = self
            .database
            .execute(
                "UPDATE observations SET acknowledged = 1 WHERE message_id = ?1",
                [&message_id],
            )
            .map_err(error)?;
        if changed == 0 {
            return Err("unknown acknowledgement".into());
        }
        Ok(AdapterMessage::Acknowledged { message_id })
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let adapter_id = match args.as_slice() {
        [_, _] => fan::ADAPTER.to_owned(),
        [_, _, flag, id] if flag == "--adapter-id" => {
            id.to_str().ok_or("adapter ID must be UTF-8")?.to_owned()
        }
        _ => return Err(
            "usage: arcs-simulated-fan <profile.json> <adapter-state.sqlite> [--adapter-id <id>]"
                .into(),
        ),
    };
    let mut simulator = Simulator::open(
        fan::Profile::load(std::path::Path::new(&args[0]))?,
        std::path::Path::new(&args[1]),
        arcs_adapter_protocol::AdapterId(adapter_id),
    )?;
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    serve(&mut simulator, &mut input, &mut output).map_err(|error| error.to_string())
}

impl Adapter for Simulator {
    fn manifest(&self) -> arcs_adapter_protocol::AdapterManifest {
        fan::manifest_with_id(self.adapter_id.clone())
    }

    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        match Simulator::poll(self).map_err(AdapterError::rejected)? {
            AdapterMessage::Observation {
                message_id,
                observation,
            } => Ok(Some(PendingObservation {
                message_id,
                observation,
            })),
            AdapterMessage::Idle => Ok(None),
            _ => Err(AdapterError::rejected("unexpected simulator poll result")),
        }
    }

    fn acknowledge(&mut self, message_id: &str) -> Result<(), AdapterError> {
        self.ack(message_id.into())
            .map(|_| ())
            .map_err(AdapterError::rejected)
    }

    fn request(&mut self, invocation: RequestInvocation) -> Result<RequestResponse, AdapterError> {
        match self.fetch(invocation).map_err(AdapterError::rejected)? {
            AdapterMessage::RequestResult { response } => Ok(response),
            _ => Err(AdapterError::rejected(
                "unexpected simulator request result",
            )),
        }
    }

    fn execute(&mut self, invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        match Simulator::execute(self, invocation).map_err(AdapterError::rejected)? {
            AdapterMessage::ExecutionResult { response } => Ok(response),
            _ => Err(AdapterError::rejected(
                "unexpected simulator execution result",
            )),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("simulated fan: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod installation_tests {
    use super::*;
    use arcs_adapter_protocol::{AdapterId, VersionId};
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "fan-installation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn profile() -> fan::Profile {
        serde_json::from_str(fan::DEFAULT_PROFILE).unwrap()
    }
    #[test]
    fn custom_identity_is_durable_and_foreign_requests_never_reach_the_journal() {
        let directory = Directory::new();
        let path = directory.0.join("fan.sqlite");
        let mut simulator =
            Simulator::open(profile(), &path, AdapterId("fan.left".into())).unwrap();
        assert_eq!(simulator.manifest().adapter_id.0, "fan.left");
        let request = RequestInvocation {
            invocation_id: "same-local-id".into(),
            capability: CapabilityRef::new("fan.right", "fan.read"),
            request_version_id: VersionId("request-1".into()),
            request_schema_id: SchemaId(fan::FAN_REQUEST.into()),
            subject: arcs_adapter_protocol::SubjectId("fan-01".into()),
            request_payload: json!({"device_id":"fan-01"}),
            response_schema_id: SchemaId(fan::FAN_STATE.into()),
            max_response_bytes: 4096,
        };
        assert!(simulator.fetch(request).is_err());
        assert_eq!(
            simulator
                .database
                .query_row("SELECT COUNT(*) FROM invocations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(simulator);
        assert!(Simulator::open(profile(), &path, AdapterId("fan.right".into())).is_err());
        assert!(Simulator::open(profile(), &path, AdapterId("fan.left".into())).is_ok());
    }
    #[test]
    fn a_legacy_default_installation_cannot_be_reassigned_to_a_custom_id() {
        let directory = Directory::new();
        let path = directory.0.join("legacy.sqlite");
        Connection::open(&path).unwrap().execute_batch("CREATE TABLE fan_state(singleton INTEGER PRIMARY KEY,device_id TEXT NOT NULL,enabled INTEGER NOT NULL,actuation_count INTEGER NOT NULL);INSERT INTO fan_state VALUES(1,'fan-01',1,7);").unwrap();
        assert!(Simulator::open(profile(), &path, AdapterId("fan.custom".into())).is_err());
        let simulator = Simulator::open(profile(), &path, AdapterId(fan::ADAPTER.into())).unwrap();
        assert_eq!(state(&simulator.database).unwrap().actuation_count, 7);
        assert_eq!(
            simulator
                .database
                .query_row("SELECT adapter_id FROM adapter_installation", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            fan::ADAPTER
        );
    }
}

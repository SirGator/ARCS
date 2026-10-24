use arcs_adapter_protocol::{
    ActInvocation, ActResponse, AdapterManifest, CapabilityId, CapabilityRef, ObservationMessage,
    RequestInvocation, RequestResponse, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError, PendingObservation};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, de::DeserializeOwned};
use std::path::Path;

use crate::{
    CANDIDATE_SCHEMA, ExecutionResult, MoveCommand, POSE_SCHEMA, PoseQuery, Profile, QUERY_SCHEMA,
    RESULT_SCHEMA, Telemetry,
};

fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}
fn rejected(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::rejected(error.to_string())
}

pub struct SimulatedDrone {
    profile: Profile,
    database: Connection,
}

fn state(database: &Connection) -> Result<Telemetry, AdapterError> {
    let value: String = database
        .query_row(
            "SELECT state_json FROM drone_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(unavailable)?;
    serde_json::from_str(&value).map_err(unavailable)
}
fn enqueue(database: &Connection, telemetry: &Telemetry) -> Result<(), AdapterError> {
    let message = ObservationMessage {
        capability_id: CapabilityId("pose.observe".into()),
        external_subject: Some(telemetry.drone_id.clone()),
        external_reference: format!("drone:{}:pose:{}", telemetry.drone_id, telemetry.revision),
        payload: serde_json::to_value(telemetry).map_err(unavailable)?,
    };
    database
        .execute(
            "INSERT INTO observations (message_id,message_json) VALUES (?1,?2)",
            params![
                format!("pose:{}", telemetry.revision),
                serde_json::to_string(&message).map_err(unavailable)?
            ],
        )
        .map_err(unavailable)?;
    Ok(())
}
fn cached<T: DeserializeOwned>(
    database: &Connection,
    id: &str,
    input: &str,
) -> Result<Option<T>, AdapterError> {
    let existing: Option<(String, String)> = database
        .query_row(
            "SELECT input_json,response_json FROM invocations WHERE invocation_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(unavailable)?;
    existing
        .map(|(previous, response)| {
            if previous != input {
                return Err(rejected("invocation identity conflict"));
            }
            serde_json::from_str(&response).map_err(unavailable)
        })
        .transpose()
}
fn remember(
    database: &Connection,
    id: &str,
    input: &str,
    response: &impl serde::Serialize,
) -> Result<(), AdapterError> {
    database
        .execute(
            "INSERT INTO invocations VALUES (?1,?2,?3)",
            params![
                id,
                input,
                serde_json::to_string(response).map_err(unavailable)?
            ],
        )
        .map_err(unavailable)?;
    Ok(())
}

impl SimulatedDrone {
    pub fn open(profile: Profile, path: &Path) -> Result<Self, AdapterError> {
        profile.validate().map_err(rejected)?;
        let identity = serde_json::to_string(&profile).map_err(rejected)?;
        let mut database = Connection::open(path).map_err(unavailable)?;
        database.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS installation (singleton INTEGER PRIMARY KEY CHECK(singleton=1),profile_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS drone_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1),state_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS invocations (invocation_id TEXT PRIMARY KEY,input_json TEXT NOT NULL,response_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS observations (sequence INTEGER PRIMARY KEY AUTOINCREMENT,message_id TEXT NOT NULL UNIQUE,message_json TEXT NOT NULL,acknowledged INTEGER NOT NULL DEFAULT 0);").map_err(unavailable)?;
        let transaction = database
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO installation VALUES (1,?1)",
                [&identity],
            )
            .map_err(unavailable)?;
        let existing: String = transaction
            .query_row(
                "SELECT profile_json FROM installation WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(unavailable)?;
        if existing != identity {
            return Err(rejected("database profile identity conflict"));
        }
        if inserted == 1 {
            let initial = Telemetry {
                drone_id: profile.drone_id.clone(),
                pose: profile.initial_pose,
                height_m: profile.room.flight_height_m,
                revision: 0,
            };
            transaction
                .execute(
                    "INSERT INTO drone_state VALUES (1,?1)",
                    [serde_json::to_string(&initial).map_err(unavailable)?],
                )
                .map_err(unavailable)?;
            enqueue(&transaction, &initial)?;
        }
        let current = state(&transaction)?;
        if current.drone_id != profile.drone_id
            || current.height_m != profile.room.flight_height_m
            || current.revision > i64::MAX as u64
        {
            return Err(rejected("invalid persisted drone identity"));
        }
        profile.room.validate_pose(current.pose).map_err(rejected)?;
        transaction.commit().map_err(unavailable)?;
        Ok(Self { profile, database })
    }
}

impl Adapter for SimulatedDrone {
    fn manifest(&self) -> AdapterManifest {
        self.profile.manifest()
    }
    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        let pending:Option<(String,String)>=self.database.query_row("SELECT message_id,message_json FROM observations WHERE acknowledged=0 ORDER BY sequence LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(unavailable)?;
        pending
            .map(|(message_id, json)| {
                Ok(PendingObservation {
                    message_id,
                    observation: serde_json::from_str(&json).map_err(unavailable)?,
                })
            })
            .transpose()
    }
    fn acknowledge(&mut self, message_id: &str) -> Result<(), AdapterError> {
        let changed = self
            .database
            .execute(
                "UPDATE observations SET acknowledged=1 WHERE message_id=?1",
                [message_id],
            )
            .map_err(unavailable)?;
        if changed == 0 {
            return Err(rejected("unknown observation ACK"));
        }
        Ok(())
    }
    fn request(&mut self, invocation: RequestInvocation) -> Result<RequestResponse, AdapterError> {
        if invocation.capability != CapabilityRef::new(&self.profile.adapter_id.0, "pose.read")
            || invocation.request_schema_id.0 != QUERY_SCHEMA
            || invocation.response_schema_id.0 != POSE_SCHEMA
            || invocation.invocation_id.trim().is_empty()
            || invocation.request_version_id.0.trim().is_empty()
        {
            return Err(rejected("pose request contract mismatch"));
        }
        let query: PoseQuery =
            serde_json::from_value(invocation.request_payload.clone()).map_err(rejected)?;
        if query.drone_id != self.profile.drone_id {
            return Err(rejected("unknown drone"));
        }
        let input = serde_json::to_string(&("request", &invocation)).map_err(rejected)?;
        let transaction = self
            .database
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        if let Some(response) = cached(&transaction, &invocation.invocation_id, &input)? {
            return Ok(response);
        }
        let current = state(&transaction)?;
        let payload = serde_json::to_value(&current).map_err(unavailable)?;
        if serde_json::to_vec(&payload).map_err(unavailable)?.len() > invocation.max_response_bytes
        {
            return Err(rejected("pose response limit exceeded"));
        }
        let response = RequestResponse {
            invocation_id: invocation.invocation_id.clone(),
            external_reference: format!("drone:{}:pose:{}", current.drone_id, current.revision),
            payload,
        };
        remember(&transaction, &invocation.invocation_id, &input, &response)?;
        transaction.commit().map_err(unavailable)?;
        Ok(response)
    }
    fn execute(&mut self, invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Materialized {
            target_version: String,
            approval_version: String,
            capability: CapabilityRef,
            input_schema_id: SchemaId,
            payload: MoveCommand,
        }
        if invocation.capability != CapabilityRef::new(&self.profile.adapter_id.0, "drone.move")
            || invocation.action_schema_id.0 != "arcs.action.v1"
            || invocation.result_schema_id.0 != RESULT_SCHEMA
            || invocation.invocation_id.trim().is_empty()
            || invocation.action_version_id.0.trim().is_empty()
        {
            return Err(rejected("move action contract mismatch"));
        }
        let action: Materialized =
            serde_json::from_value(invocation.payload.clone()).map_err(rejected)?;
        if action.capability != invocation.capability
            || action.input_schema_id.0 != CANDIDATE_SCHEMA
            || action.target_version.trim().is_empty()
            || action.approval_version.trim().is_empty()
            || action.payload.drone_id != self.profile.drone_id
        {
            return Err(rejected("materialized move mismatch"));
        }
        let input = serde_json::to_string(&("execute", &invocation)).map_err(rejected)?;
        let transaction = self
            .database
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        if let Some(response) = cached(&transaction, &invocation.invocation_id, &input)? {
            return Ok(response);
        }
        let mut current = state(&transaction)?;
        if current.revision != action.payload.expected_revision {
            return Err(rejected("stale pose revision"));
        }
        current.pose = self
            .profile
            .room
            .moved(current.pose, action.payload.movement)
            .map_err(rejected)?;
        current.revision = current
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| rejected("revision overflow"))?;
        let response = ActResponse {
            invocation_id: invocation.invocation_id.clone(),
            external_reference: format!("drone:{}:pose:{}", current.drone_id, current.revision),
            result_payload: serde_json::to_value(ExecutionResult {
                action_version: invocation.action_version_id,
                result: current.clone(),
            })
            .map_err(unavailable)?,
        };
        transaction
            .execute(
                "UPDATE drone_state SET state_json=?1 WHERE singleton=1",
                [serde_json::to_string(&current).map_err(unavailable)?],
            )
            .map_err(unavailable)?;
        enqueue(&transaction, &current)?;
        remember(&transaction, &invocation.invocation_id, &input, &response)?;
        transaction.commit().map_err(unavailable)?;
        Ok(response)
    }
}

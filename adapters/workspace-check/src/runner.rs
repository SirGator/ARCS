//! Run only named operator-defined checks. The journal commits intent before spawning.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use arcs_adapter_protocol::{
    ActInvocation, ActResponse, AdapterId, AdapterManifest, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, SchemaId,
};
use arcs_adapter_sdk::{Adapter, AdapterError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CAPABILITY: &str = "check.run";
const CANDIDATE: &str = "arcs.workspace_check_candidate.v1";
const RESULT: &str = "arcs.execution_result.workspace_check.v1";
const SUPERVISOR: &str = "/usr/bin/timeout";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    workspace: PathBuf,
    checks: BTreeMap<String, Check>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandSelection {
    check: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializedAction {
    target_version: String,
    approval_version: String,
    capability: CapabilityRef,
    input_schema_id: SchemaId,
    payload: CommandSelection,
}

pub(crate) struct Runner {
    profile: Profile,
    db: Connection,
    _lock: File,
}

impl Runner {
    pub(crate) fn open(profile_path: &Path, journal: &Path) -> Result<Self, String> {
        let profile_path = profile_path.canonicalize().map_err(message)?;
        let mut profile: Profile =
            serde_json::from_slice(&fs::read(&profile_path).map_err(message)?).map_err(message)?;
        let base = profile_path.parent().ok_or("profile directory missing")?;
        if !Path::new(SUPERVISOR).is_file() {
            return Err("workspace checks require /usr/bin/timeout".into());
        }
        profile.workspace = base
            .join(&profile.workspace)
            .canonicalize()
            .map_err(message)?;
        if !profile.workspace.is_dir() || !(1..=64).contains(&profile.checks.len()) {
            return Err("a workspace directory and 1–64 named checks are required".into());
        }
        for (name, check) in &mut profile.checks {
            // Keep aliases such as rustup's cargo proxy as the executable name.
            check.program = base.join(&check.program);
            if !identifier(name)
                || !check.program.is_file()
                || !(1..=600_000).contains(&check.timeout_ms)
                || check.args.iter().any(|argument| argument.contains('\0'))
            {
                return Err(format!("invalid check definition: {name}"));
            }
        }

        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(journal)
            .map_err(message)?;
        let journal = journal.canonicalize().map_err(message)?;
        let mut lock_name = journal.as_os_str().to_owned();
        lock_name.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_name)
            .map_err(message)?;
        lock.try_lock()
            .map_err(|error| format!("workspace check journal is in use: {error}"))?;
        let db = Connection::open(journal).map_err(message)?;
        let runner = Self {
            profile,
            db,
            _lock: lock,
        };
        runner
            .manifest()
            .validate()
            .map_err(|error| format!("{error:?}"))?;
        runner.bind_profile()?;
        Ok(runner)
    }

    fn bind_profile(&self) -> Result<(), String> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS profile (singleton INTEGER PRIMARY KEY CHECK(singleton=1), definition TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS checks (invocation_id TEXT PRIMARY KEY, input TEXT NOT NULL, result TEXT);",
        ).map_err(message)?;
        let configured = serde_json::to_string(&self.profile).map_err(message)?;
        self.db
            .execute("INSERT OR IGNORE INTO profile VALUES (1,?1)", [&configured])
            .map_err(message)?;
        let bound: String = self
            .db
            .query_row(
                "SELECT definition FROM profile WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(message)?;
        if bound != configured {
            return Err("journal belongs to a different workspace check profile".into());
        }
        Ok(())
    }

    fn execute_named(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterError> {
        let action: MaterializedAction =
            serde_json::from_value(invocation.payload.clone()).map_err(rejected)?;
        if invocation.capability != CapabilityRef::new(&self.profile.adapter_id.0, CAPABILITY)
            || invocation.action_schema_id.0 != "arcs.action.v1"
            || invocation.result_schema_id.0 != RESULT
            || !identifier(&invocation.invocation_id)
            || invocation.action_version_id.0.trim().is_empty()
            || action.target_version.trim().is_empty()
            || action.approval_version.trim().is_empty()
            || action.capability != invocation.capability
            || action.input_schema_id.0 != CANDIDATE
        {
            return Err(AdapterError::rejected(
                "workspace check action contract mismatch",
            ));
        }
        let name = &action.payload.check;
        let check = self
            .profile
            .checks
            .get(name)
            .ok_or_else(|| AdapterError::rejected("workspace check is not configured"))?;
        let id = &invocation.invocation_id;
        let input = serde_json::to_string(invocation).map_err(rejected)?;
        let receipt: Option<(String, Option<String>)> = self
            .db
            .query_row(
                "SELECT input,result FROM checks WHERE invocation_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(unavailable)?;

        let result = if let Some((saved_input, result)) = receipt {
            if saved_input != input {
                return Err(AdapterError::rejected(
                    "workspace check invocation identity conflict",
                ));
            }
            match result {
                Some(result) => serde_json::from_str(&result).map_err(unavailable)?,
                None => self.save_result(id, json!({"check":name,"status":"interrupted"}))?,
            }
        } else {
            self.db
                .execute(
                    "INSERT INTO checks(invocation_id,input) VALUES (?1,?2)",
                    params![id, input],
                )
                .map_err(unavailable)?;
            self.save_result(id, execute_check(&self.profile.workspace, name, check))?
        };
        Ok(ActResponse {
            invocation_id: id.clone(),
            external_reference: format!("workspace-check:{id}"),
            result_payload: json!({"action_version":invocation.action_version_id,"result":result}),
        })
    }

    fn save_result(&self, id: &str, result: Value) -> Result<Value, AdapterError> {
        let encoded = serde_json::to_string(&result).map_err(unavailable)?;
        let updated = self
            .db
            .execute(
                "UPDATE checks SET result=?2 WHERE invocation_id=?1 AND result IS NULL",
                params![id, encoded],
            )
            .map_err(unavailable)?;
        if updated != 1 {
            return Err(AdapterError::unavailable(
                "workspace check result checkpoint changed",
            ));
        }
        Ok(result)
    }
}

impl Adapter for Runner {
    fn manifest(&self) -> AdapterManifest {
        AdapterManifest {
            protocol_version: 1,
            adapter_id: self.profile.adapter_id.clone(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![CapabilityDescriptor {
                id: CapabilityId(CAPABILITY.into()),
                description: None,
                contract: CapabilityContract::Act {
                    accepts: vec![SchemaId(CANDIDATE.into())],
                    emits: vec![SchemaId(RESULT.into())],
                    idempotent: true,
                },
                required_permissions: vec!["workspace.check".into()],
            }],
        }
    }

    fn execute(&mut self, invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        self.execute_named(&invocation)
    }
}

fn execute_check(workspace: &Path, name: &str, check: &Check) -> Value {
    let started = Instant::now();
    // The supervisor maintains the deadline even if the adapter process is lost.
    let timeout = format!(
        "{}.{:03}s",
        check.timeout_ms / 1000,
        check.timeout_ms % 1000
    );
    let process = Command::new(SUPERVISOR)
        .args(["--signal=KILL", "--", &timeout])
        .arg(&check.program)
        .args(&check.args)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let (status, exit_code) = match process {
        Ok(exit) if exit.success() => ("passed", exit.code()),
        Ok(exit) if exit.code().is_none() || matches!(exit.code(), Some(124 | 137)) => {
            ("timed_out_or_killed", exit.code())
        }
        Ok(exit) if matches!(exit.code(), Some(125..=127)) => ("runner_error", exit.code()),
        Ok(exit) => ("failed", exit.code()),
        Err(_) => ("runner_error", None),
    };
    let mut result =
        json!({"check":name,"status":status,"duration_ms":started.elapsed().as_millis() as u64});
    if let Some(code) = exit_code {
        result["exit_code"] = json!(code);
    }
    result
}

fn identifier(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn rejected(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::rejected(message(error))
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(message(error))
}

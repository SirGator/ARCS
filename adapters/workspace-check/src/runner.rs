//! Run only named operator-defined checks. The journal commits intent before spawning.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::diagnostics::{self, Diagnostics};
use crate::git_edit::{self, EditProfile};

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
const REVISION_CAPABILITY: &str = "check.revision";
const REVISION_CANDIDATE: &str = "arcs.workspace_revision_check_candidate.v1";
const REVISION_RESULT: &str = "arcs.execution_result.workspace_revision_check.v1";
const REVISION_PERMISSION: &str = "workspace.check_revision";
const DIAGNOSTIC_CAPABILITY: &str = "check.revision.diagnostic";
const DIAGNOSTIC_RESULT: &str = "arcs.execution_result.workspace_revision_check.v2";
const DIAGNOSTIC_PERMISSION: &str = "workspace.check_revision_diagnostics";
const SUPERVISOR: &str = "/usr/bin/timeout";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    adapter_id: AdapterId,
    workspace: PathBuf,
    checks: BTreeMap<String, Check>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edit: Option<EditProfile>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    revision_checks: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    diagnostic_revision_checks: BTreeSet<String>,
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
struct RevisionSelection {
    check: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializedAction<T> {
    target_version: String,
    approval_version: String,
    capability: CapabilityRef,
    input_schema_id: SchemaId,
    payload: T,
}

pub(crate) struct Runner {
    profile: Profile,
    db: Connection,
    journal: PathBuf,
    _lock: File,
}

impl Runner {
    pub(crate) fn open(profile_path: &Path, journal: &Path) -> Result<Self, String> {
        let profile_path = profile_path.canonicalize().map_err(message)?;
        let mut profile: Profile =
            serde_json::from_slice(&fs::read(&profile_path).map_err(message)?).map_err(message)?;
        let base = profile_path.parent().ok_or("profile directory missing")?;
        if !profile.checks.is_empty() && !Path::new(SUPERVISOR).is_file() {
            return Err("workspace checks require /usr/bin/timeout".into());
        }
        profile.workspace = base
            .join(&profile.workspace)
            .canonicalize()
            .map_err(message)?;
        if !profile.workspace.is_dir()
            || profile.checks.len() > 64
            || (profile.checks.is_empty() && profile.edit.is_none())
        {
            return Err("a workspace directory and configured capabilities are required".into());
        }
        if let Some(edit) = &profile.edit {
            edit.validate(&profile.workspace)?;
        }
        if (!profile.revision_checks.is_empty() || !profile.diagnostic_revision_checks.is_empty())
            && (profile.edit.is_none()
                || profile
                    .revision_checks
                    .union(&profile.diagnostic_revision_checks)
                    .any(|name| !profile.checks.contains_key(name)))
        {
            return Err(
                "revision checks require a configured Git edit ref and named checks".into(),
            );
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
        let db = Connection::open(&journal).map_err(message)?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(message)?;
        let runner = Self {
            profile,
            db,
            journal,
            _lock: lock,
        };
        runner
            .manifest()
            .validate()
            .map_err(|error| format!("{error:?}"))?;
        runner.bind_profile()?;
        if runner.profile.edit.is_some() {
            git_edit::initialize(&runner.db)?;
        }
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

    fn materialized<T: serde::de::DeserializeOwned>(
        &self,
        invocation: &ActInvocation,
        capability: &str,
        candidate_schema: &str,
        result_schema: &str,
    ) -> Result<T, AdapterError> {
        let action: MaterializedAction<T> =
            serde_json::from_value(invocation.payload.clone()).map_err(rejected)?;
        if invocation.capability != CapabilityRef::new(&self.profile.adapter_id.0, capability)
            || invocation.action_schema_id.0 != "arcs.action.v1"
            || invocation.result_schema_id.0 != result_schema
            || !identifier(&invocation.invocation_id)
            || invocation.action_version_id.0.trim().is_empty()
            || action.target_version.trim().is_empty()
            || action.approval_version.trim().is_empty()
            || action.capability != invocation.capability
            || action.input_schema_id.0 != candidate_schema
        {
            return Err(AdapterError::rejected("workspace action contract mismatch"));
        }
        Ok(action.payload)
    }

    fn execute_named(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterError> {
        let action: CommandSelection =
            self.materialized(invocation, CAPABILITY, CANDIDATE, RESULT)?;
        self.execute_check(invocation, &action.check, None, false)
    }

    fn execute_revision(
        &self,
        invocation: &ActInvocation,
        diagnostic: bool,
    ) -> Result<ActResponse, AdapterError> {
        let (capability, schema, allowed) = if diagnostic {
            (
                DIAGNOSTIC_CAPABILITY,
                DIAGNOSTIC_RESULT,
                &self.profile.diagnostic_revision_checks,
            )
        } else {
            (
                REVISION_CAPABILITY,
                REVISION_RESULT,
                &self.profile.revision_checks,
            )
        };
        let action: RevisionSelection =
            self.materialized(invocation, capability, REVISION_CANDIDATE, schema)?;
        if !allowed.contains(&action.check) || !git_edit::object_id(&action.revision) {
            return Err(AdapterError::rejected(
                "workspace revision check requires a configured check and a full commit ID",
            ));
        }
        self.execute_check(
            invocation,
            &action.check,
            Some(&action.revision),
            diagnostic,
        )
    }

    fn execute_check(
        &self,
        invocation: &ActInvocation,
        name: &str,
        revision: Option<&str>,
        diagnostic: bool,
    ) -> Result<ActResponse, AdapterError> {
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
                None => self.save_result(
                    id,
                    journal_result(
                        check_result(name, "interrupted", revision),
                        None,
                        diagnostic,
                    ),
                )?,
            }
        } else {
            let snapshot = revision
                .map(|commit| {
                    self.profile
                        .edit
                        .as_ref()
                        .expect("revision checks require an edit profile")
                        .snapshot(&self.profile.workspace, commit)
                })
                .transpose()?;
            let working_dir = snapshot.as_ref().map_or(
                self.profile.workspace.as_path(),
                git_edit::TemporaryDirectory::path,
            );
            self.db
                .execute(
                    "INSERT INTO checks(invocation_id,input) VALUES (?1,?2)",
                    params![id, input],
                )
                .map_err(unavailable)?;
            let (result, output) = run_check(working_dir, name, check, revision, diagnostic);
            self.save_result(id, journal_result(result, output, diagnostic))?
        };
        let mut payload = if diagnostic {
            result
        } else {
            json!({"result":result})
        };
        payload
            .as_object_mut()
            .ok_or_else(|| AdapterError::unavailable("saved check receipt must be an object"))?
            .insert("action_version".into(), json!(invocation.action_version_id));
        Ok(ActResponse {
            invocation_id: id.clone(),
            external_reference: format!("workspace-check:{id}"),
            result_payload: payload,
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
        let mut capabilities = Vec::new();
        if !self.profile.checks.is_empty() {
            capabilities.push(act_capability(
                CAPABILITY,
                CANDIDATE,
                RESULT,
                "workspace.check",
            ));
        }
        if self.profile.edit.is_some() {
            capabilities.push(act_capability(
                git_edit::CAPABILITY,
                git_edit::CANDIDATE,
                git_edit::RESULT,
                "workspace.edit",
            ));
        }
        if !self.profile.revision_checks.is_empty() {
            capabilities.push(act_capability(
                REVISION_CAPABILITY,
                REVISION_CANDIDATE,
                REVISION_RESULT,
                REVISION_PERMISSION,
            ));
        }
        if !self.profile.diagnostic_revision_checks.is_empty() {
            capabilities.push(act_capability(
                DIAGNOSTIC_CAPABILITY,
                REVISION_CANDIDATE,
                DIAGNOSTIC_RESULT,
                DIAGNOSTIC_PERMISSION,
            ));
        }
        AdapterManifest {
            protocol_version: 1,
            adapter_id: self.profile.adapter_id.clone(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities,
        }
    }

    fn execute(&mut self, invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        match invocation.capability.capability_id.0.as_str() {
            CAPABILITY => return self.execute_named(&invocation),
            REVISION_CAPABILITY => return self.execute_revision(&invocation, false),
            DIAGNOSTIC_CAPABILITY => return self.execute_revision(&invocation, true),
            git_edit::CAPABILITY => {}
            _ => return Err(AdapterError::rejected("unknown workspace capability")),
        }
        let edit = self
            .profile
            .edit
            .as_ref()
            .ok_or_else(|| AdapterError::rejected("workspace editing is not configured"))?;
        let replacement = self.materialized(
            &invocation,
            git_edit::CAPABILITY,
            git_edit::CANDIDATE,
            git_edit::RESULT,
        )?;
        let result = edit.execute(
            &self.profile.workspace,
            &self.journal,
            &self.db,
            &invocation,
            replacement,
        )?;
        Ok(ActResponse {
            invocation_id: invocation.invocation_id.clone(),
            external_reference: format!("workspace-edit:{}", invocation.invocation_id),
            result_payload: json!({"action_version":invocation.action_version_id,"result":result}),
        })
    }
}

fn act_capability(id: &str, accepts: &str, emits: &str, permission: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId(id.into()),
        description: None,
        contract: CapabilityContract::Act {
            accepts: vec![SchemaId(accepts.into())],
            emits: vec![SchemaId(emits.into())],
            idempotent: true,
        },
        required_permissions: vec![permission.into()],
    }
}

fn run_check(
    workspace: &Path,
    name: &str,
    check: &Check,
    revision: Option<&str>,
    diagnostic: bool,
) -> (Value, Option<Diagnostics>) {
    let started = Instant::now();
    // The supervisor maintains the deadline even if the adapter process is lost.
    let timeout = format!(
        "{}.{:03}s",
        check.timeout_ms / 1000,
        check.timeout_ms % 1000
    );
    let mut command = Command::new(SUPERVISOR);
    if revision.is_some() {
        for (name, _) in
            std::env::vars_os().filter(|(name, _)| name.as_encoded_bytes().starts_with(b"GIT_"))
        {
            command.env_remove(name);
        }
        command.env(
            "GIT_CEILING_DIRECTORIES",
            workspace.parent().expect("snapshot directory has a parent"),
        );
    }
    command
        .args(["--signal=KILL", "--", &timeout])
        .arg(&check.program)
        .args(&check.args)
        .current_dir(workspace)
        .stdin(Stdio::null());
    let process = if diagnostic {
        diagnostics::run(&mut command).map(|(status, output)| (status, Some(output)))
    } else {
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| (status, None))
    };
    let (process, output) = match process {
        Ok((status, output)) => (Some(status), output),
        Err(_) => (None, None),
    };
    let (status, exit_code) = match process {
        Some(exit) if exit.success() => ("passed", exit.code()),
        Some(exit) if exit.code().is_none() || matches!(exit.code(), Some(124 | 137)) => {
            ("timed_out_or_killed", exit.code())
        }
        Some(exit) if matches!(exit.code(), Some(125..=127)) => ("runner_error", exit.code()),
        Some(exit) => ("failed", exit.code()),
        None => ("runner_error", None),
    };
    let mut result = check_result(name, status, revision);
    result["duration_ms"] = json!(started.elapsed().as_millis() as u64);
    if let Some(code) = exit_code {
        result["exit_code"] = json!(code);
    }
    let output = output.filter(|_| matches!(status, "failed" | "timed_out_or_killed"));
    (result, output)
}

// Existing profiles keep the original bare result in their journals.
fn journal_result(result: Value, output: Option<Diagnostics>, diagnostic: bool) -> Value {
    if !diagnostic {
        return result;
    }
    let mut payload = json!({"result":result});
    if let Some(output) = output {
        payload["diagnostics"] = json!(output);
    }
    payload
}

fn check_result(name: &str, status: &str, revision: Option<&str>) -> Value {
    let mut result = json!({"check":name,"status":status});
    if let Some(commit) = revision {
        result["revision"] = json!(commit);
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

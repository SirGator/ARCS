//! Version a named file replacement and publish it through an atomic local Git ref update.
mod repository;
mod snapshot;

pub(crate) use repository::TemporaryDirectory;

use repository::Git;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use arcs_adapter_protocol::ActInvocation;
use arcs_adapter_sdk::AdapterError;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const CAPABILITY: &str = "edit.apply";
pub(crate) const CANDIDATE: &str = "arcs.workspace_edit_candidate.v1";
pub(crate) const RESULT: &str = "arcs.execution_result.workspace_edit.v1";
const LIMIT: usize = 32_768;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryScope {
    reference: String,
    files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replacement {
    base_commit: String,
    path: String,
    replacement: String,
}

struct EditJournal<'a> {
    db: &'a Connection,
    id: &'a str,
}

struct SavedEdit {
    input: String,
    commit: String,
    prepared_at: Option<i64>,
    result: Option<String>,
}

impl RepositoryScope {
    pub(crate) fn validate(&self, workspace: &Path) -> Result<(), String> {
        if !self
            .reference
            .strip_prefix("refs/arcs/")
            .is_some_and(segment)
            || !(1..=128).contains(&self.files.len())
            || self.files.iter().any(|path| !editable_path(path))
            || self
                .files
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.files.len()
        {
            return Err(
                "workspace Git scope needs a local refs/arcs/ reference and distinct file paths"
                    .into(),
            );
        }
        let git = Git { workspace };
        git.text(&["check-ref-format", &self.reference], None)?;
        let root = git.text(&["rev-parse", "--show-toplevel"], None)?;
        if Path::new(&root) != workspace {
            return Err("workspace Git scope requires the root of a local Git repository".into());
        }
        let commit = git.revision(&self.reference)?;
        if git.text(&["cat-file", "-t", &commit], None)? != "commit" {
            return Err("workspace reference must point to a commit".into());
        }
        Ok(())
    }

    pub(crate) fn snapshot(
        &self,
        workspace: &Path,
        revision: &str,
    ) -> Result<TemporaryDirectory, AdapterError> {
        let git = Git { workspace };
        self.validate_revision(&git, revision)?;
        snapshot::export(&git, revision)
    }

    pub(crate) fn source(
        &self,
        workspace: &Path,
        revision: &str,
        path: &str,
    ) -> Result<String, AdapterError> {
        if !self.files.iter().any(|allowed| allowed == path) {
            return Err(AdapterError::rejected(
                "workspace source path is not configured",
            ));
        }
        let git = Git { workspace };
        self.validate_revision(&git, revision)?;
        git.read_text(revision, path).map(|(_, text)| text)
    }

    fn validate_revision(&self, git: &Git<'_>, revision: &str) -> Result<(), AdapterError> {
        let tip = git.revision(&self.reference).map_err(unavailable)?;
        if !object_id(revision)
            || revision.len() != tip.len()
            || !git
                .text(&["cat-file", "-t", revision], None)
                .is_ok_and(|kind| kind == "commit")
            || !git.contains(&tip, revision).map_err(unavailable)?
        {
            return Err(AdapterError::rejected(
                "workspace revision must be a commit reachable from the configured ref",
            ));
        }
        Ok(())
    }

    pub(crate) fn execute(
        &self,
        workspace: &Path,
        journal_path: &Path,
        db: &Connection,
        invocation: &ActInvocation,
        edit: Replacement,
    ) -> Result<Value, AdapterError> {
        if !object_id(&edit.base_commit)
            || !self.files.contains(&edit.path)
            || edit.replacement.len() > LIMIT
            || edit.replacement.contains('\0')
        {
            return Err(AdapterError::rejected(
                "workspace edit exceeds its file, base or text policy",
            ));
        }
        let journal = EditJournal {
            db,
            id: &invocation.invocation_id,
        };
        let input = serde_json::to_string(invocation).map_err(unavailable)?;
        let git = Git { workspace };
        let (commit, prepared_at) = if let Some(saved) = journal.load()? {
            if saved.input != input {
                return Err(AdapterError::rejected(
                    "workspace edit invocation identity conflict",
                ));
            }
            if let Some(result) = saved.result {
                return serde_json::from_str(&result).map_err(unavailable);
            }
            if !object_id(&saved.commit) {
                return Err(AdapterError::unavailable(
                    "saved workspace commit ID is invalid",
                ));
            }
            let prepared_at = saved.prepared_at.ok_or_else(|| {
                AdapterError::unavailable("saved workspace edit timestamp is missing")
            })?;
            (saved.commit, prepared_at)
        } else {
            let tip = git.revision(&self.reference).map_err(unavailable)?;
            if tip != edit.base_commit {
                return journal.insert_result(&input, &edit, "conflict", &tip);
            }
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(unavailable)?
                .as_secs();
            let prepared_at = i64::try_from(seconds).map_err(unavailable)?;
            let commit = git.prepare_commit(journal_path, journal.id, &edit, prepared_at)?;
            if commit == edit.base_commit {
                return journal.insert_result(&input, &edit, "unchanged", &commit);
            }
            journal.stage(&input, &commit, prepared_at)?;
            (commit, prepared_at)
        };

        let mut tip = git.revision(&self.reference).map_err(unavailable)?;
        if tip == edit.base_commit {
            // The object can be pruned while the reference still points at
            // the base during a long outage. Rebuild the same object before CAS.
            if !git.has_commit(&commit).map_err(unavailable)?
                && git.prepare_commit(journal_path, journal.id, &edit, prepared_at)? != commit
            {
                return Err(AdapterError::unavailable(
                    "rebuilt workspace commit differs from its checkpoint",
                ));
            }
            let updated = git
                .run(
                    &[
                        "update-ref",
                        "--no-deref",
                        &self.reference,
                        &commit,
                        &edit.base_commit,
                    ],
                    None,
                    None,
                )
                .map_err(unavailable)?;
            if updated.status.success() {
                return journal.finish(&edit, "applied", &commit);
            }
            tip = git.revision(&self.reference).map_err(unavailable)?;
            if tip == edit.base_commit {
                return Err(AdapterError::unavailable(
                    "local workspace reference could not be updated",
                ));
            }
        }
        // A crash after the ref update can leave a committed edit without its receipt.
        if git.has_commit(&commit).map_err(unavailable)?
            && git.contains(&tip, &commit).map_err(unavailable)?
        {
            journal.finish(&edit, "applied", &commit)
        } else {
            journal.finish(&edit, "conflict", &tip)
        }
    }
}

pub(crate) fn initialize(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS edits (
            invocation_id TEXT PRIMARY KEY, input TEXT NOT NULL, commit_id TEXT NOT NULL,
            prepared_at INTEGER, result TEXT, CHECK(result IS NOT NULL OR prepared_at IS NOT NULL)
        );",
    )
    .map_err(|error| error.to_string())
}

impl EditJournal<'_> {
    fn load(&self) -> Result<Option<SavedEdit>, AdapterError> {
        self.db
            .query_row(
                "SELECT input,commit_id,prepared_at,result FROM edits WHERE invocation_id=?1",
                [self.id],
                |row| {
                    Ok(SavedEdit {
                        input: row.get(0)?,
                        commit: row.get(1)?,
                        prepared_at: row.get(2)?,
                        result: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(unavailable)
    }

    fn stage(&self, input: &str, commit: &str, prepared_at: i64) -> Result<(), AdapterError> {
        self.db
            .execute(
                "INSERT INTO edits(invocation_id,input,commit_id,prepared_at) VALUES(?1,?2,?3,?4)",
                params![self.id, input, commit, prepared_at],
            )
            .map_err(unavailable)?;
        Ok(())
    }

    fn insert_result(
        &self,
        input: &str,
        edit: &Replacement,
        status: &str,
        commit: &str,
    ) -> Result<Value, AdapterError> {
        let result = receipt(edit, status, commit);
        let encoded = serde_json::to_string(&result).map_err(unavailable)?;
        self.db
            .execute(
                "INSERT INTO edits(invocation_id,input,commit_id,result) VALUES(?1,?2,?3,?4)",
                params![self.id, input, commit, encoded],
            )
            .map_err(unavailable)?;
        Ok(result)
    }

    fn finish(
        &self,
        edit: &Replacement,
        status: &str,
        commit: &str,
    ) -> Result<Value, AdapterError> {
        let result = receipt(edit, status, commit);
        let encoded = serde_json::to_string(&result).map_err(unavailable)?;
        let changed = self
            .db
            .execute(
                "UPDATE edits SET result=?2 WHERE invocation_id=?1 AND result IS NULL",
                params![self.id, encoded],
            )
            .map_err(unavailable)?;
        if changed != 1 {
            return Err(AdapterError::unavailable(
                "workspace edit receipt checkpoint changed",
            ));
        }
        Ok(result)
    }
}

fn receipt(edit: &Replacement, status: &str, commit: &str) -> Value {
    json!({"path":edit.path,"base_commit":edit.base_commit,"status":status,"commit":commit})
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | ".." | ".git")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
fn editable_path(path: &str) -> bool {
    path.len() <= 256 && path.split('/').all(segment)
}
pub(crate) fn object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::unavailable(error.to_string())
}

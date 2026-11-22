//! Bubblewrap owns the namespace boundary for revision checks.
mod resources;

use resources::ResourceLimits;
pub(crate) use resources::ResourceUsage;

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{self, Diagnostics};
use crate::git_edit::TemporaryDirectory;

pub(crate) const CAPABILITY: &str = "check.revision.isolated";
pub(crate) const PERMISSION: &str = "workspace.check_revision_isolated";
pub(crate) const RESOURCE_RESULT: &str = "arcs.execution_result.workspace_revision_check.v3";
const DIAGNOSTIC_RESULT: &str = "arcs.execution_result.workspace_revision_check.v2";
const RUNTIME_ROOTS: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64"];
const SCRATCH_BYTES: &str = "67108864";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sandbox {
    pub program: PathBuf,
    pub checks: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resources: Option<ResourceLimits>,
}

pub(crate) struct CheckJournal<'a> {
    pub db: &'a Connection,
    pub invocation: &'a str,
}

pub(crate) struct IsolatedRun {
    pub exit: ExitStatus,
    pub diagnostics: Diagnostics,
    pub reported_exit: bool,
    pub resources: Option<ResourceUsage>,
}

pub(crate) fn validate_program(program: &Path) -> Result<(), String> {
    let resolved = program.canonicalize().map_err(|error| error.to_string())?;
    let in_runtime = |path: &Path| RUNTIME_ROOTS.iter().any(|root| path.starts_with(root));
    if !in_runtime(program) || !in_runtime(&resolved) {
        return Err("isolated check executable must be in a mounted system directory".into());
    }
    Ok(())
}

impl Sandbox {
    pub(crate) fn prepare(&mut self, base: &Path) -> Result<(), String> {
        self.program = base
            .join(&self.program)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if let Some(limits) = &mut self.resources {
            limits.parent = base
                .join(&limits.parent)
                .canonicalize()
                .map_err(|error| error.to_string())?;
            limits.validate()?;
        }
        Ok(())
    }

    pub(crate) fn recover_resources(&self, journal: &Connection) -> io::Result<()> {
        if let Some(limits) = &self.resources {
            limits.recover(journal)?;
        }
        Ok(())
    }

    pub(crate) fn result_schema(&self) -> &'static str {
        if self.resources.is_some() {
            RESOURCE_RESULT
        } else {
            DIAGNOSTIC_RESULT
        }
    }

    pub(crate) fn run(
        &self,
        supervisor: &mut Command,
        snapshot: &Path,
        program: &Path,
        arguments: &[String],
        journal: CheckJournal<'_>,
    ) -> io::Result<IsolatedRun> {
        let parent = snapshot
            .parent()
            .ok_or_else(|| io::Error::other("snapshot has no parent"))?;
        let status_dir = TemporaryDirectory::create(parent, "check-sandbox")?;
        let status_file = status_dir.path().join("status.jsonl");

        let group = self
            .resources
            .as_ref()
            .map(|limits| limits.create(journal.db, journal.invocation))
            .transpose()?;

        // The fixed launcher only opens the status FD and joins the cgroup.
        // Paths and command arguments stay separate argv entries.
        supervisor
            .env_clear()
            .args([
                "/bin/sh",
                "-c",
                r#"exec 3>"$1" || exit 125
if [ -n "$2" ]; then printf '0\n' >"$2" || exit 125; fi
shift 2
exec "$@""#,
                "arcs-bwrap",
            ])
            .arg(&status_file)
            .arg(
                group
                    .as_ref()
                    .map(|group| group.membership())
                    .unwrap_or_default(),
            )
            .arg(&self.program)
            .args([
                "--unshare-all",
                "--unshare-user",
                "--disable-userns",
                "--die-with-parent",
                "--new-session",
                "--cap-drop",
                "ALL",
                "--clearenv",
                "--setenv",
                "PATH",
                "/usr/bin:/bin",
                "--setenv",
                "HOME",
                "/tmp",
                "--setenv",
                "TMPDIR",
                "/tmp",
                "--setenv",
                "LC_ALL",
                "C.UTF-8",
                "--ro-bind",
                "/usr",
                "/usr",
            ]);
        for root in &RUNTIME_ROOTS[1..] {
            supervisor.args(["--ro-bind-try", root, root]);
        }
        supervisor
            .args([
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--size",
                SCRATCH_BYTES,
                "--tmpfs",
                "/tmp",
                "--ro-bind",
            ])
            .arg(snapshot)
            .args([
                "/workspace",
                "--chdir",
                "/workspace",
                "--remount-ro",
                "/",
                "--json-status-fd",
                "3",
                "--",
            ])
            .arg(program)
            .args(arguments);

        let (exit, diagnostics) = diagnostics::run(supervisor)?;
        let reported = exit
            .code()
            .is_some_and(|code| recorded_exit(&status_file) == Some(i64::from(code)));
        Ok(IsolatedRun {
            exit,
            diagnostics,
            reported_exit: reported,
            resources: group.map(|group| group.finish()).transpose()?,
        })
    }
}

fn recorded_exit(path: &Path) -> Option<i64> {
    // A namespace startup error can report a child PID without reporting a
    // workload exit. Never count that as an executed check.
    let reader = File::open(path).ok()?;
    let mut bytes = Vec::new();
    reader.take(4096).read_to_end(&mut bytes).ok()?;
    let mut reports = serde_json::Deserializer::from_slice(&bytes).into_iter::<Value>();
    let startup = reports.next()?.ok()?;
    if startup["child-pid"].as_u64()? == 0 {
        return None;
    }
    let exit = reports.next()?.ok()?;
    if reports.next().is_some() {
        return None;
    }
    exit["exit-code"].as_i64()
}

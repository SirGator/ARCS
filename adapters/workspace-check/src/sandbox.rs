//! Bubblewrap owns the namespace boundary for revision checks.
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{self, Diagnostics};
use crate::git_edit::TemporaryDirectory;

pub(crate) const CAPABILITY: &str = "check.revision.isolated";
pub(crate) const PERMISSION: &str = "workspace.check_revision_isolated";
const RUNTIME_ROOTS: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64"];
const SCRATCH_BYTES: &str = "67108864";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sandbox {
    pub program: PathBuf,
    pub checks: BTreeSet<String>,
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
    pub(crate) fn run(
        &self,
        supervisor: &mut Command,
        snapshot: &Path,
        program: &Path,
        arguments: &[String],
    ) -> io::Result<(ExitStatus, Diagnostics, bool)> {
        let parent = snapshot
            .parent()
            .ok_or_else(|| io::Error::other("snapshot has no parent"))?;
        let status_dir = TemporaryDirectory::create(parent, "check-sandbox")?;
        let status_file = status_dir.path().join("status.jsonl");

        // The shell only opens the supervisor status FD. Paths and command
        // arguments are separate argv entries and never interpolated.
        supervisor
            .env_clear()
            .args([
                "/bin/sh",
                "-c",
                "exec 3>\"$1\" || exit 125\nshift\nexec \"$@\"",
                "arcs-bwrap",
            ])
            .arg(&status_file)
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
        Ok((exit, diagnostics, reported))
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

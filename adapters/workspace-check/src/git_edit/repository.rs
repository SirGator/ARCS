//! Git object construction uses an isolated index and local plumbing commands.
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use arcs_adapter_sdk::AdapterError;

use super::{LIMIT, Replacement, object_id, unavailable};

pub(super) struct Git<'a> {
    pub(super) workspace: &'a Path,
}

impl Git<'_> {
    pub(super) fn command(&self, index: Option<&Path>) -> Command {
        let mut command = Command::new("/usr/bin/git");
        command
            .current_dir(self.workspace)
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "ARCS")
            .env("GIT_AUTHOR_EMAIL", "arcs@localhost")
            .env("GIT_COMMITTER_NAME", "ARCS")
            .env("GIT_COMMITTER_EMAIL", "arcs@localhost")
            .args([
                "--literal-pathspecs",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.fsync=loose-object,reference",
                "-c",
                "core.fsyncMethod=fsync",
                "-c",
                "commit.gpgSign=false",
            ])
            .stdin(Stdio::null());
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        command
    }

    pub(super) fn run(
        &self,
        args: &[&str],
        index: Option<&Path>,
        input: Option<&[u8]>,
    ) -> Result<Output, String> {
        let mut command = self.command(index);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let written = match input {
            Some(bytes) => child
                .stdin
                .take()
                .expect("piped git input")
                .write_all(bytes),
            None => Ok(()),
        };
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        written.map_err(|error| error.to_string())?;
        Ok(output)
    }

    pub(super) fn text(&self, args: &[&str], index: Option<&Path>) -> Result<String, String> {
        output_text(self.run(args, index, None)?, args[0])
    }

    pub(super) fn revision(&self, reference: &str) -> Result<String, String> {
        let revision = self.text(
            &["rev-parse", "--verify", "--end-of-options", reference],
            None,
        )?;
        if !object_id(&revision) {
            return Err("invalid Git object ID in workspace reference".into());
        }
        Ok(revision)
    }

    pub(super) fn has_commit(&self, commit: &str) -> Result<bool, String> {
        Ok(self
            .run(
                &["cat-file", "-e", &format!("{commit}^{{commit}}")],
                None,
                None,
            )?
            .status
            .success())
    }

    pub(super) fn contains(&self, tip: &str, commit: &str) -> Result<bool, String> {
        if tip == commit {
            return Ok(true);
        }
        let output = self.run(&["merge-base", "--is-ancestor", commit, tip], None, None)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err("could not verify the workspace revision ancestry".into()),
        }
    }

    pub(super) fn read_text(
        &self,
        revision: &str,
        path: &str,
    ) -> Result<(&'static str, String), AdapterError> {
        let entry = self
            .text(&["ls-tree", revision, "--", path], None)
            .map_err(unavailable)?;
        let (header, actual_path) = entry
            .split_once('\t')
            .ok_or_else(|| AdapterError::rejected("configured file is absent from the revision"))?;
        let fields: Vec<_> = header.split_ascii_whitespace().collect();
        let [mode @ ("100644" | "100755"), "blob", previous] = fields.as_slice() else {
            return Err(AdapterError::rejected(
                "workspace source requires a regular file",
            ));
        };
        if actual_path != path {
            return Err(AdapterError::rejected(
                "workspace file differs from the configured path",
            ));
        }
        let bytes = self
            .text(&["cat-file", "-s", previous], None)
            .map_err(unavailable)?;
        if !bytes.parse::<usize>().is_ok_and(|size| size <= LIMIT) {
            return Err(AdapterError::rejected(
                "workspace file exceeds the source size limit",
            ));
        }
        let original = self
            .run(&["cat-file", "blob", previous], None, None)
            .map_err(unavailable)?;
        if !original.status.success() {
            return Err(AdapterError::unavailable(
                "workspace blob could not be read",
            ));
        }
        if original.stdout.contains(&0) {
            return Err(AdapterError::rejected(
                "workspace source requires a UTF-8 text file",
            ));
        }
        let text = String::from_utf8(original.stdout)
            .map_err(|_| AdapterError::rejected("workspace source requires a UTF-8 text file"))?;
        Ok((
            if *mode == "100755" {
                "100755"
            } else {
                "100644"
            },
            text,
        ))
    }

    pub(super) fn prepare_commit(
        &self,
        journal: &Path,
        invocation: &str,
        edit: &Replacement,
        prepared_at: i64,
    ) -> Result<String, AdapterError> {
        let (mode, original) = self.read_text(&edit.base_commit, &edit.path)?;
        if original.as_bytes() == edit.replacement.as_bytes() {
            return Ok(edit.base_commit.clone());
        }
        let blob = output_text(
            self.run(
                &["hash-object", "-w", "--stdin"],
                None,
                Some(edit.replacement.as_bytes()),
            )
            .map_err(unavailable)?,
            "hash-object",
        )
        .map_err(unavailable)?;
        let temp = TemporaryDirectory::create(
            journal.parent().expect("canonical journal has a parent"),
            "edit-index",
        )
        .map_err(unavailable)?;
        let index = temp.path().join("index");
        self.text(&["read-tree", &edit.base_commit], Some(&index))
            .map_err(unavailable)?;
        let cache = format!("{mode},{blob},{}", edit.path);
        self.text(&["update-index", "--cacheinfo", &cache], Some(&index))
            .map_err(unavailable)?;
        let tree = self
            .text(&["write-tree"], Some(&index))
            .map_err(unavailable)?;
        let message = format!("Apply ARCS workspace edit {invocation}");
        let date = format!("@{prepared_at} +0000");
        let output = self
            .command(None)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args([
                "-c",
                "i18n.commitEncoding=UTF-8",
                "commit-tree",
                &tree,
                "-p",
                &edit.base_commit,
                "-m",
                &message,
            ])
            .output()
            .map_err(unavailable)?;
        output_text(output, "commit-tree").map_err(unavailable)
    }
}

fn output_text(output: Output, operation: &str) -> Result<String, String> {
    if !output.status.success() {
        return Err(format!(
            "workspace git {operation} failed ({})",
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim_end_matches('\n').to_owned())
        .map_err(|error| error.to_string())
}

pub(crate) struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    pub(super) fn create(parent: &Path, purpose: &str) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = parent.join(format!(
                ".arcs-{purpose}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

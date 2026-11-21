use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};
use std::path::Path;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use arcs_adapter_sdk::AdapterError;

use super::repository::{Git, TemporaryDirectory};
use super::{object_id, unavailable};

const MAX_TREE_LIST: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 50_000;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

pub(super) fn export(git: &Git<'_>, revision: &str) -> Result<TemporaryDirectory, AdapterError> {
    let mut listing = Vec::new();
    read_git(
        git.command(None)
            .args(["ls-tree", "-r", "-z", "-l", revision]),
        false,
        |_, output| {
            output
                .take(MAX_TREE_LIST + 1)
                .read_to_end(&mut listing)
                .map_err(unavailable)?;
            if listing.len() as u64 > MAX_TREE_LIST {
                return Err(AdapterError::rejected(
                    "workspace revision tree listing is too large",
                ));
            }
            Ok(())
        },
    )?;
    let temp_root = std::env::temp_dir().canonicalize().map_err(unavailable)?;
    let snapshot = TemporaryDirectory::create(&temp_root, "check-tree").map_err(unavailable)?;
    read_git(
        git.command(None).arg("cat-file").arg("--batch"),
        true,
        |input, output| {
            materialize(
                input.expect("piped git batch input"),
                output,
                snapshot.path(),
                &listing,
            )
        },
    )?;
    Ok(snapshot)
}

fn read_git(
    command: &mut Command,
    batch: bool,
    consume: impl FnOnce(
        Option<&mut ChildStdin>,
        &mut BufReader<ChildStdout>,
    ) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    let mut child = command
        .stdin(if batch { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(unavailable)?;
    let mut output = BufReader::new(child.stdout.take().expect("piped git output"));
    let mut input = child.stdin.take();
    let consumed = consume(input.as_mut(), &mut output);
    drop(input);
    drop(output);
    if consumed.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(unavailable)?;
    consumed?;
    if !status.success() {
        return Err(unavailable(format!(
            "workspace snapshot Git process ended with {status}"
        )));
    }
    Ok(())
}

fn materialize(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
    root: &Path,
    listing: &[u8],
) -> Result<(), AdapterError> {
    if listing.is_empty() {
        return Ok(());
    }
    let listing = listing.strip_suffix(b"\0").ok_or_else(invalid_tree)?;
    let mut total = 0;
    let mut header = Vec::new();
    for (index, entry) in listing.split(|byte| *byte == 0).enumerate() {
        let (metadata, path) = entry.split_at(
            entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(invalid_tree)?,
        );
        let path = &path[1..];
        let metadata = std::str::from_utf8(metadata).map_err(unavailable)?;
        let mut fields = metadata.split_ascii_whitespace();
        let mode = match (fields.next(), fields.next()) {
            (Some("100644"), Some("blob")) => 0o644,
            (Some("100755"), Some("blob")) => 0o755,
            _ => {
                return Err(AdapterError::rejected(
                    "workspace revision must contain only regular files",
                ));
            }
        };
        let id = fields
            .next()
            .filter(|id| object_id(id))
            .ok_or_else(invalid_tree)?;
        let size: u64 = fields
            .next()
            .and_then(|size| size.parse().ok())
            .ok_or_else(invalid_tree)?;
        if fields.next().is_some()
            || path.split(|byte| *byte == b'/').any(|part| {
                matches!(part, b"" | b"." | b"..") || part.eq_ignore_ascii_case(b".git")
            })
        {
            return Err(invalid_tree());
        }
        if index >= MAX_FILES || size > MAX_FILE_BYTES || total + size > MAX_TOTAL_BYTES {
            return Err(AdapterError::rejected(
                "workspace revision exceeds the snapshot file or byte limit",
            ));
        }
        total += size;

        writeln!(input, "{id}").map_err(unavailable)?;
        header.clear();
        output
            .by_ref()
            .take(128)
            .read_until(b'\n', &mut header)
            .map_err(unavailable)?;
        if header != format!("{id} blob {size}\n").as_bytes() {
            return Err(unavailable(
                "workspace snapshot blob differs from its tree entry",
            ));
        }
        let path = root.join(OsStr::from_bytes(path));
        fs::create_dir_all(path.parent().expect("path belongs to snapshot"))
            .map_err(unavailable)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(path)
            .map_err(unavailable)?;
        let written = io::copy(&mut output.by_ref().take(size), &mut file).map_err(unavailable)?;
        let mut separator = [0];
        output.read_exact(&mut separator).map_err(unavailable)?;
        if written != size || separator != *b"\n" {
            return Err(unavailable("workspace snapshot blob is incomplete"));
        }
    }
    Ok(())
}

fn invalid_tree() -> AdapterError {
    AdapterError::rejected("workspace revision contains an invalid Git tree entry")
}

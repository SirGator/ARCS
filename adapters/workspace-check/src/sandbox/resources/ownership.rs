//! Durable ownership is recorded before the launcher can join a cgroup.
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};

use super::{counter, write_control};

const NAME_PREFIX: &str = "arcs-resource-";
const CLEANUP_WINDOW: Duration = Duration::from_secs(2);

#[derive(PartialEq, Eq)]
struct Identity {
    device: String,
    inode: String,
}

impl Identity {
    fn read(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev().to_string(),
            inode: metadata.ino().to_string(),
        }
    }
}

pub(super) struct GroupLease<'a> {
    pub directory: PathBuf,
    db: &'a Connection,
    invocation: &'a str,
    identity: Option<Identity>,
    released: bool,
}

pub(super) fn recover(parent: &Path, db: &Connection) -> io::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS check_cgroups (
            invocation_id TEXT PRIMARY KEY REFERENCES checks(invocation_id),
            name TEXT NOT NULL UNIQUE,
            device TEXT,
            inode TEXT,
            CHECK ((device IS NULL) = (inode IS NULL))
        );",
    )
    .map_err(journal_error)?;

    // Collect first so cleanup can release journal rows independently.
    let entries = {
        let mut statement = db
            .prepare(
                "SELECT invocation_id,name,device,inode FROM check_cgroups ORDER BY invocation_id",
            )
            .map_err(journal_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(journal_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(journal_error)?
    };
    for (invocation, name, device, inode) in entries {
        if !valid_name(&name) {
            return Err(invalid("invalid resource cgroup name in check journal"));
        }
        let identity = match (device, inode) {
            (Some(device), Some(inode))
                if device.parse::<u64>().is_ok() && inode.parse::<u64>().is_ok() =>
            {
                Some(Identity { device, inode })
            }
            (None, None) => None,
            _ => return Err(invalid("invalid resource cgroup identity in check journal")),
        };
        remove_group(&parent.join(&name), identity.as_ref()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("recover resource cgroup {name}: {error}"),
            )
        })?;
        release(db, &invocation)?;
    }
    Ok(())
}

impl<'a> GroupLease<'a> {
    pub(super) fn create(
        parent: &Path,
        db: &'a Connection,
        invocation: &'a str,
    ) -> io::Result<Self> {
        let mut nonce = [0; 16];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let name = format!("{NAME_PREFIX}{:032x}", u128::from_ne_bytes(nonce));
        let directory = parent.join(&name);
        db.execute(
            "INSERT INTO check_cgroups(invocation_id,name) VALUES (?1,?2)",
            params![invocation, name],
        )
        .map_err(journal_error)?;
        if let Err(error) = fs::create_dir(&directory) {
            // A pre-existing group has never belonged to this invocation.
            release(db, invocation)?;
            return Err(error);
        }
        Ok(Self {
            directory,
            db,
            invocation,
            identity: None,
            released: false,
        })
    }

    pub(super) fn confirm(&mut self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.directory)?;
        if !metadata.is_dir() {
            return Err(invalid("resource cgroup identity changed"));
        }
        let identity = Identity::read(&metadata);
        let updated = self
            .db
            .execute(
                "UPDATE check_cgroups SET device=?2,inode=?3 WHERE invocation_id=?1 AND device IS NULL",
                params![self.invocation, identity.device, identity.inode],
            )
            .map_err(journal_error)?;
        if updated != 1 {
            return Err(invalid("resource cgroup ownership checkpoint changed"));
        }
        self.identity = Some(identity);
        Ok(())
    }

    pub(super) fn stop(&self) -> io::Result<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| invalid("unconfirmed resource cgroup"))?;
        if !matches_group(&self.directory, Some(identity))? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "resource cgroup disappeared",
            ));
        }
        let deadline = Instant::now() + CLEANUP_WINDOW;
        if populated(&self.directory)? {
            write_control(&self.directory, "cgroup.kill", "1")?;
        }
        while populated(&self.directory)? {
            wait_for_empty(deadline)?;
        }
        Ok(())
    }

    pub(super) fn remove(mut self) -> io::Result<()> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        remove_group(&self.directory, self.identity.as_ref())?;
        release(self.db, self.invocation)?;
        self.released = true;
        Ok(())
    }
}

impl Drop for GroupLease<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.cleanup();
        }
    }
}

fn remove_group(directory: &Path, identity: Option<&Identity>) -> io::Result<()> {
    let deadline = Instant::now() + CLEANUP_WINDOW;
    // A reservation alone permits removing an empty directory, never signaling tasks.
    while matches_group(directory, identity)? {
        if identity.is_some() && populated(directory)? {
            write_control(directory, "cgroup.kill", "1")?;
            wait_for_empty(deadline)?;
            continue;
        }
        // rmdir is atomic with migration into the group. If a launcher was
        // interrupted just before joining, retry the kill/remove sequence.
        match fs::remove_dir(directory) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::ResourceBusy && identity.is_some() => {
                wait_for_empty(deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn matches_group(directory: &Path, identity: Option<&Identity>) -> io::Result<bool> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || identity.is_some_and(|expected| Identity::read(&metadata) != *expected)
            {
                return Err(invalid("resource cgroup identity changed"));
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn populated(directory: &Path) -> io::Result<bool> {
    Ok(counter(
        &fs::read_to_string(directory.join("cgroup.events"))?,
        "populated",
    )? != 0)
}

fn wait_for_empty(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "sandbox cgroup did not empty",
        ));
    }
    std::thread::sleep(Duration::from_millis(10));
    Ok(())
}

fn release(db: &Connection, invocation: &str) -> io::Result<()> {
    db.execute(
        "DELETE FROM check_cgroups WHERE invocation_id=?1",
        [invocation],
    )
    .map_err(journal_error)?;
    Ok(())
}

fn valid_name(name: &str) -> bool {
    name.strip_prefix(NAME_PREFIX).is_some_and(|nonce| {
        nonce.len() == 32
            && nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn journal_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("resource cgroup journal: {error}"))
}

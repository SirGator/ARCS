//! Read a bounded prefix from each stream while the supervisor runs.
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use serde::Serialize;

const PREFIX_BYTES: usize = 512;

#[derive(Serialize)]
pub(crate) struct Diagnostics {
    stdout: Output,
    stderr: Output,
}

#[derive(Serialize)]
struct Output {
    text: String,
    truncated: bool,
}

impl Diagnostics {
    pub(crate) fn setup_error(error: &io::Error) -> Self {
        let message = format!("isolated check setup or cleanup: {error}");
        let prefix = message.as_bytes();
        Self {
            stdout: Output::new(Vec::new(), false),
            stderr: Output::new(
                prefix[..prefix.len().min(PREFIX_BYTES)].to_vec(),
                prefix.len() > PREFIX_BYTES,
            ),
        }
    }
}

impl Output {
    fn new(mut prefix: Vec<u8>, truncated: bool) -> Self {
        for byte in &mut prefix {
            if byte.is_ascii_control() && !matches!(*byte, b'\n' | b'\r' | b'\t') {
                *byte = b'.';
            }
        }
        Self {
            text: String::from_utf8_lossy(&prefix).into_owned(),
            truncated,
        }
    }
}

struct Capture {
    reader: UnixStream,
    prefix: Vec<u8>,
    closed: bool,
    truncated: bool,
}

pub(crate) fn run(command: &mut Command) -> io::Result<(ExitStatus, Diagnostics)> {
    let (mut stdout, stdout_writer) = Capture::pair()?;
    let (mut stderr, stderr_writer) = Capture::pair()?;
    let child = command.stdout(stdout_writer).stderr(stderr_writer).spawn();
    // Command keeps its copies of the write ends after spawn.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = child?;
    loop {
        let read_stdout = stdout.poll();
        let read_stderr = stderr.poll();
        if let Some(status) = child.try_wait()? {
            return Ok((
                status,
                Diagnostics {
                    stdout: stdout.finish(),
                    stderr: stderr.finish(),
                },
            ));
        }
        if !(read_stdout || read_stderr) {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Capture {
    fn pair() -> io::Result<(Self, Stdio)> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        Ok((
            Self {
                reader,
                prefix: Vec::with_capacity(PREFIX_BYTES),
                closed: false,
                truncated: false,
            },
            Stdio::from(OwnedFd::from(writer)),
        ))
    }

    fn poll(&mut self) -> bool {
        if self.closed {
            return false;
        }
        let mut buffer = [0; 8192];
        match self.reader.read(&mut buffer) {
            Ok(0) => {
                self.closed = true;
                false
            }
            Ok(count) => {
                let keep = count.min(PREFIX_BYTES - self.prefix.len());
                self.prefix.extend_from_slice(&buffer[..keep]);
                self.truncated |= keep < count;
                true
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => true,
            Err(_) => {
                self.truncated = true;
                self.closed = true;
                false
            }
        }
    }

    fn finish(mut self) -> Output {
        // Drain only what is immediately available. A background child may
        // hold the descriptor open after its supervised parent has exited.
        for _ in 0..8 {
            if !self.poll() {
                break;
            }
        }
        Output::new(self.prefix, self.truncated || !self.closed)
    }
}

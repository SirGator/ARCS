use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arcs_adapter_protocol::{
    CapabilityRef, OutputInvocation, SchemaId, VersionId,
    stdio::{AdapterMessage, Envelope, HostMessage, WIRE_VERSION, read_frame, write_frame},
};
use arcs_adapter_sdk::Adapter;
use arcs_file_output::FileOutput;
use serde_json::{Value, json};

struct Fixture {
    directory: PathBuf,
    profile: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-file-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self {
            directory,
            profile: json!({"adapter_id":"output.test","directory":"reports"}),
        };
        fixture.save();
        fixture
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn save(&self) {
        std::fs::write(
            self.path("profile.json"),
            serde_json::to_vec(&self.profile).unwrap(),
        )
        .unwrap();
    }
    fn open(&self) -> Result<FileOutput, String> {
        FileOutput::open(&self.path("profile.json"), &self.path("journal.sqlite"))
    }
    fn files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.path("reports"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn invocation() -> OutputInvocation {
    OutputInvocation {
        invocation_id: "output:test-1".into(),
        capability: CapabilityRef::new("output.test", "report.write"),
        request_version_id: VersionId("request-v1".into()),
        approval_version_id: VersionId("approval-v1".into()),
        artifact_version_id: VersionId("report-v1".into()),
        artifact_schema_id: SchemaId("arcs.report.v1".into()),
        subject: None,
        payload: json!({"title":"Report","body":"Example"}),
        result_schema_id: SchemaId("arcs.delivery_receipt.file.v1".into()),
        max_result_bytes: 4096,
    }
}

#[test]
fn journal_and_file_are_replayed_after_restart_without_duplicate_delivery() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    let first = adapter.output(invocation()).unwrap();
    assert_eq!(adapter.output(invocation()).unwrap(), first);
    drop(adapter);
    let mut adapter = fixture.open().unwrap();
    assert_eq!(adapter.output(invocation()).unwrap(), first);
    assert_eq!(fixture.files().len(), 1);
    let file: OutputInvocation =
        serde_json::from_slice(&std::fs::read(&fixture.files()[0]).unwrap()).unwrap();
    assert_eq!(file, invocation());
}

#[test]
fn conflicting_identity_never_overwrites_the_existing_report() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    adapter.output(invocation()).unwrap();
    let original = std::fs::read(&fixture.files()[0]).unwrap();
    let mut changed = invocation();
    changed.payload["body"] = json!("different");
    assert!(adapter.output(changed).is_err());
    let mut changed = invocation();
    changed.approval_version_id.0 = "another-approval".into();
    assert!(adapter.output(changed).is_err());
    assert_eq!(std::fs::read(&fixture.files()[0]).unwrap(), original);
    assert_eq!(fixture.files().len(), 1);
}

#[test]
fn published_file_recovers_a_crash_before_journal_commit() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    let original = adapter.output(invocation()).unwrap();
    drop(adapter);
    let journal = rusqlite::Connection::open(fixture.path("journal.sqlite")).unwrap();
    journal.execute("DELETE FROM deliveries", []).unwrap();
    drop(journal);
    let mut adapter = fixture.open().unwrap();
    assert_eq!(adapter.output(invocation()).unwrap(), original);
    assert_eq!(fixture.files().len(), 1);
    let journal = rusqlite::Connection::open(fixture.path("journal.sqlite")).unwrap();
    let count: i64 = journal
        .query_row("SELECT count(*) FROM deliveries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn unrelated_existing_file_and_bad_paths_are_rejected() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    let encoded: String = invocation()
        .invocation_id
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let path = fixture.path(&format!("reports/output-{encoded}.json"));
    std::fs::write(&path, b"operator data").unwrap();
    assert!(adapter.output(invocation()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"operator data");
    let mut bad = invocation();
    bad.invocation_id = "../other".into();
    assert!(adapter.output(bad).is_err());
    assert_eq!(fixture.files().len(), 1);
}

#[test]
fn journal_cannot_be_retargeted_to_a_different_installation_or_directory() {
    let mut fixture = Fixture::new();
    drop(fixture.open().unwrap());
    fixture.profile["directory"] = json!("elsewhere");
    fixture.save();
    assert!(fixture.open().is_err());
    fixture.profile["directory"] = json!("reports");
    fixture.profile["adapter_id"] = json!("other");
    fixture.save();
    assert!(fixture.open().is_err());
}

#[test]
fn receipt_limits_and_invalid_reports_fail_before_publication() {
    let fixture = Fixture::new();
    let mut adapter = fixture.open().unwrap();
    let mut limited = invocation();
    limited.max_result_bytes = 1;
    assert!(adapter.output(limited).is_err());
    let mut invalid = invocation();
    invalid.payload["title"] = json!("\n");
    assert!(adapter.output(invalid).is_err());
    assert!(fixture.files().is_empty());
}

struct Process {
    child: Child,
    input: std::process::ChildStdin,
    output: BufReader<std::process::ChildStdout>,
}
impl Process {
    fn start(fixture: &Fixture) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_arcs-file-output"))
            .arg(fixture.path("profile.json"))
            .arg(fixture.path("journal.sqlite"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut process = Self {
            child,
            input,
            output,
        };
        process.send(HostMessage::Hello);
        assert!(matches!(process.receive(), AdapterMessage::Manifest { .. }));
        process
    }
    fn send(&mut self, message: HostMessage) {
        write_frame(
            &mut self.input,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: "test".into(),
                message,
            },
        )
        .unwrap();
    }
    fn receive(&mut self) -> AdapterMessage {
        read_frame::<Envelope<AdapterMessage>>(&mut self.output)
            .unwrap()
            .message
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn lost_process_response_is_recovered_with_one_published_file() {
    let fixture = Fixture::new();
    let mut process = Process::start(&fixture);
    process.send(HostMessage::Output {
        invocation: invocation(),
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.files().is_empty() {
        assert!(Instant::now() < deadline, "output never published");
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(process);
    let mut process = Process::start(&fixture);
    process.send(HostMessage::Output {
        invocation: invocation(),
    });
    assert!(matches!(
        process.receive(),
        AdapterMessage::OutputResult { .. }
    ));
    assert_eq!(fixture.files().len(), 1);
}

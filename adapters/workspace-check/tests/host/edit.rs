use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const EDIT_REF: &str = "refs/arcs/test";
const EDIT_SCHEMA: &str = "arcs.workspace_edit_candidate.v1";
const EDIT_RECEIPT: &str = "arcs.execution_result.workspace_edit.v1";
const REVISION_SCHEMA: &str = "arcs.workspace_revision_check_candidate.v1";
const REVISION_RECEIPT: &str = "arcs.execution_result.workspace_revision_check.v1";
const ORIGINAL: &str = "def add(a, b):\n    return a - b\n";
const FIXED: &str = "def add(a, b):\n    return a + b\n";

fn git(project: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .current_dir(project)
        .args([
            "-c",
            "user.name=Test Maintainer",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn enable_edits(f: &Fixture, project: &Path) -> String {
    git(project, &["init", "-q"]);
    fs::write(project.join("calculator.py"), ORIGINAL).unwrap();
    fs::write(project.join("untouched.txt"), "same content\n").unwrap();
    let executable = project.join("verify");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    git(project, &["add", "."]);
    git(project, &["commit", "-q", "-m", "Starting project"]);
    let base = git(project, &["rev-parse", "HEAD"]).trim().to_owned();
    git(project, &["update-ref", EDIT_REF, &base]);

    let mut host = f.read("host.json");
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    host["schemas"].as_array_mut().unwrap().extend([
        json!(schemas.join("edit-candidate.schema.json")),
        json!(schemas.join("edit-execution.schema.json")),
    ]);
    f.write("host.json", host);
    let mut profile = f.read("profile.json");
    profile["edit"] = json!({"reference":EDIT_REF,"files":["calculator.py","link.py"]});
    f.write("profile.json", profile);
    let mut grant = f.read("grant.json");
    grant["enabled_capabilities"] = json!(["check.run", "edit.apply"]);
    grant["granted_permissions"] = json!(["workspace.check", "workspace.edit"]);
    f.write("grant.json", grant);
    base
}

fn enable_revision_checks(f: &Fixture) {
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    let mut host = f.read("host.json");
    host["schemas"].as_array_mut().unwrap().extend([
        json!(schemas.join("revision-candidate.schema.json")),
        json!(schemas.join("revision-execution.schema.json")),
    ]);
    f.write("host.json", host);
    let mut profile = f.read("profile.json");
    profile["revision_checks"] = json!(["unit"]);
    profile["checks"]["unit"] = json!({
        "program":"/usr/bin/python3",
        "args":["-B","-c",r#"
from pathlib import Path
from calculator import add
import sys
with Path(sys.argv[1]).open('a') as runs:
    runs.write('unit\n')
assert Path('untouched.txt').read_bytes() == b'same content\n'
assert b'\r' not in Path('calculator.py').read_bytes()
assert not Path('untracked.txt').exists()
assert Path('verify').stat().st_mode & 0o100
assert add(2, 3) == 5
"#, f.0.join("revision-attempts")],
        "timeout_ms":3000
    });
    f.write("profile.json", profile);
    let mut grant = f.read("grant.json");
    grant["enabled_capabilities"] = json!(["edit.apply", "check.revision"]);
    grant["granted_permissions"] = json!(["workspace.edit", "workspace.check_revision"]);
    f.write("grant.json", grant);
}

fn revision_check(host: &Host, id: &str, check: &str, revision: &str) -> Artifact {
    propose(
        host,
        id,
        REVISION_SCHEMA,
        "check.revision",
        json!({"check":check,"revision":revision}),
    )
}

fn revision(project: &Path) -> String {
    git(project, &["rev-parse", EDIT_REF]).trim().to_owned()
}

fn edit_candidate(host: &Host, id: &str, base: &str, path: &str, replacement: &str) -> Artifact {
    propose(
        host,
        id,
        EDIT_SCHEMA,
        "edit.apply",
        json!({
            "base_commit":base,"path":path,"replacement":replacement,
        }),
    )
}

#[test]
fn an_approved_edit_recovers_the_published_commit_and_a_real_check_passes_on_its_tree() {
    let f = Fixture::new();
    let project = f.0.join("project");
    fs::write(
        project.join(".gitattributes"),
        "untouched.txt export-ignore\ncalculator.py text eol=crlf\n",
    )
    .unwrap();
    let base = enable_edits(&f, &project);
    fs::write(project.join("untracked.txt"), "mutable worktree").unwrap();
    enable_revision_checks(&f);
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();

    let proposed = edit_candidate(&host, "addition-fix", &base, "calculator.py", FIXED);
    assert!(
        host.execute_approved(&adapter, &proposed.version_id)
            .is_err()
    );
    assert_eq!(revision(&project), base);
    let action = approve_for(&host, &proposed, "edit.apply");
    let adapter_db = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let core_db = Connection::open(f.0.join("core.sqlite")).unwrap();
    adapter_db
        .execute_batch(
            "CREATE TRIGGER lose_edit_receipt BEFORE UPDATE OF result ON edits
        BEGIN SELECT RAISE(ABORT,'simulated adapter receipt rollback'); END;",
        )
        .unwrap();
    core_db.execute_batch("CREATE TRIGGER lose_failed_attempt BEFORE INSERT ON artifact_versions
        WHEN NEW.schema_id IN ('arcs.invocation.v1','arcs.invocation.v2') AND json_extract(NEW.artifact_json,'$.payload.status')='failed'
        BEGIN SELECT RAISE(ABORT,'lost failure response'); END;").unwrap();
    assert!(host.execute_approved(&adapter, &action).is_err());
    let published = revision(&project);
    assert_ne!(published, base);
    let planned: String = adapter_db
        .query_row(
            "SELECT commit_id FROM edits WHERE result IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(planned, published);
    drop(host);
    adapter_db
        .execute_batch("DROP TRIGGER lose_edit_receipt")
        .unwrap();
    core_db
        .execute_batch("DROP TRIGGER lose_failed_attempt")
        .unwrap();

    let mut host = f.open();
    let result = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(result.schema_id.0, EDIT_RECEIPT);
    assert_eq!(
        result.payload["result"],
        json!({"status":"applied","path":"calculator.py","base_commit":base,"commit":published})
    );
    assert_eq!(revision(&project), published);
    assert_eq!(
        git(&project, &["show", &format!("{published}:calculator.py")]),
        FIXED
    );
    assert_eq!(
        git(&project, &["show", &format!("{published}:untouched.txt")]),
        "same content\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("calculator.py")).unwrap(),
        ORIGINAL
    );
    assert_eq!(
        git(
            &project,
            &["rev-list", "--count", &format!("{base}..{published}")]
        )
        .trim(),
        "1"
    );
    for (label, commit, status) in [
        ("unit-before", &base, "failed"),
        ("unit-after", &published, "passed"),
    ] {
        let candidate = revision_check(&host, label, "unit", commit);
        let action = approve_for(&host, &candidate, "check.revision");
        let result = host.execute_approved(&adapter, &action).unwrap();
        assert_eq!(result.schema_id.0, REVISION_RECEIPT);
        assert_eq!(result.payload["action_version"], json!(action));
        assert_eq!(result.payload["result"]["revision"], commit.as_str());
        assert_eq!(result.payload["result"]["status"], status);
    }
    assert_eq!(
        fs::read_to_string(f.0.join("revision-attempts")).unwrap(),
        "unit\nunit\n"
    );
    host.shutdown().unwrap();
}

#[test]
fn a_stale_edit_records_conflict_and_an_identical_edit_leaves_the_commit_unchanged() {
    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();
    let unchanged = edit_candidate(&host, "identical", &base, "calculator.py", ORIGINAL);
    let action = approve_for(&host, &unchanged, "edit.apply");
    assert_eq!(
        host.execute_approved(&adapter, &action).unwrap().payload["result"]["status"],
        "unchanged"
    );
    assert_eq!(revision(&project), base);
    let edit = edit_candidate(&host, "valid", &base, "calculator.py", FIXED);
    let action = approve_for(&host, &edit, "edit.apply");
    assert_eq!(
        host.execute_approved(&adapter, &action).unwrap().payload["result"]["status"],
        "applied"
    );
    let current = revision(&project);
    let stale = edit_candidate(&host, "stale", &base, "calculator.py", ORIGINAL);
    let action = approve_for(&host, &stale, "edit.apply");
    let result = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(result.payload["result"]["status"], "conflict");
    assert_eq!(result.payload["result"]["commit"], current);
    assert_eq!(revision(&project), current);
    host.shutdown().unwrap();
}

#[test]
fn edits_refuse_unconfigured_paths_and_symlinks_even_with_an_approved_candidate() {
    for path in ["untouched.txt", "link.py"] {
        let f = Fixture::new();
        let project = f.0.join("project");
        std::os::unix::fs::symlink("calculator.py", project.join("link.py")).unwrap();
        let base = enable_edits(&f, &project);
        let mut host = f.open();
        let candidate = edit_candidate(&host, "rejected-file", &base, path, FIXED);
        let action = approve_for(&host, &candidate, "edit.apply");
        assert!(
            host.execute_approved(&AdapterId(ADAPTER.into()), &action)
                .is_err()
        );
        assert_eq!(revision(&project), base);
        drop(host);
    }
}

#[test]
fn a_prepared_edit_recreates_the_exact_commit_if_its_unpublished_object_is_pruned() {
    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();
    let proposed = edit_candidate(&host, "pruned-intent", &base, "calculator.py", FIXED);
    let action = approve_for(&host, &proposed, "edit.apply");
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    core.execute_batch("CREATE TRIGGER lose_failed_attempt BEFORE INSERT ON artifact_versions
        WHEN NEW.schema_id IN ('arcs.invocation.v1','arcs.invocation.v2') AND json_extract(NEW.artifact_json,'$.payload.status')='failed'
        BEGIN SELECT RAISE(ABORT,'lost failure response'); END;").unwrap();
    let ref_lock = project.join(".git/refs/arcs/test.lock");
    fs::write(&ref_lock, "").unwrap();
    assert!(host.execute_approved(&adapter, &action).is_err());
    assert_eq!(revision(&project), base);
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let prepared: String = journal
        .query_row(
            "SELECT commit_id FROM edits WHERE result IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(host);
    core.execute_batch("DROP TRIGGER lose_failed_attempt")
        .unwrap();
    fs::remove_file(ref_lock).unwrap();
    // Only the test's unpublished loose commit is removed. Neither ref
    // pointed to this object when the adapter returned its first reply.
    fs::remove_file(
        project
            .join(".git/objects")
            .join(&prepared[..2])
            .join(&prepared[2..]),
    )
    .unwrap();

    let mut host = f.open();
    let result = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(result.payload["result"]["commit"], prepared);
    assert_eq!(result.payload["result"]["status"], "applied");
    assert_eq!(revision(&project), prepared);
    host.shutdown().unwrap();
}

#[test]
fn a_competing_git_transaction_keeps_its_commit_when_a_prepared_edit_reaches_the_ref() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    git(&project, &["config", "core.filesRefLockTimeout", "5000"]);
    let tree = git(&project, &["rev-parse", "HEAD^{tree}"]);
    let newer = git(
        &project,
        &[
            "commit-tree",
            tree.trim(),
            "-p",
            &base,
            "-m",
            "Concurrent operator version",
        ],
    )
    .trim()
    .to_owned();
    let mut host = f.open();
    let candidate = edit_candidate(&host, "concurrent-edit", &base, "calculator.py", FIXED);
    let action = approve_for(&host, &candidate, "edit.apply");
    host.shutdown().unwrap();
    drop(host);

    let mut operator = Command::new("/usr/bin/git")
        .current_dir(&project)
        .args(["-c", "core.hooksPath=/dev/null", "update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut request = operator.stdin.take().unwrap();
    writeln!(request, "start\nupdate {EDIT_REF} {newer} {base}\nprepare").unwrap();
    let mut events = BufReader::new(operator.stdout.take().unwrap()).lines();
    assert_eq!(events.next().unwrap().unwrap(), "start: ok");
    assert_eq!(events.next().unwrap().unwrap(), "prepare: ok");

    let config = f.0.join("host.json");
    let attempt = std::thread::spawn(move || {
        let mut host = Host::open(config).unwrap();
        let result = host
            .execute_approved(&AdapterId(ADAPTER.into()), &action)
            .unwrap();
        host.shutdown().unwrap();
        result
    });
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !journal
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM edits WHERE result IS NULL)",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    writeln!(request, "commit").unwrap();
    drop(request);
    assert!(operator.wait().unwrap().success());

    let receipt = attempt.join().unwrap();
    assert_eq!(receipt.payload["result"]["status"], "conflict");
    assert_eq!(receipt.payload["result"]["commit"], newer);
    assert_eq!(revision(&project), newer);
    assert_eq!(
        git(&project, &["show", &format!("{newer}:calculator.py")]),
        ORIGINAL
    );
}

#[test]
fn revision_checks_reject_unapproved_tree_scope_and_unlisted_checks_before_starting() {
    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    fs::write(project.join("calculator.py"), FIXED).unwrap();
    git(&project, &["commit", "-qam", "Unrelated fixed version"]);
    let unrelated = git(&project, &["rev-parse", "HEAD"]);
    let blob = git(&project, &["rev-parse", "HEAD:calculator.py"]);

    std::os::unix::fs::symlink("calculator.py", project.join("link.py")).unwrap();
    git(&project, &["add", "link.py"]);
    git(&project, &["commit", "-qm", "Link in candidate tree"]);
    let symlink = git(&project, &["rev-parse", "HEAD"]);

    enable_revision_checks(&f);
    let adapter = AdapterId(ADAPTER.into());
    for (label, check, commit, expected) in [
        (
            "unlisted-command",
            "pass",
            base.as_str(),
            "configured check",
        ),
        (
            "unrelated-commit",
            "unit",
            unrelated.trim(),
            "reachable from the configured ref",
        ),
        (
            "not-a-commit",
            "unit",
            blob.trim(),
            "reachable from the configured ref",
        ),
        ("symlink-tree", "unit", symlink.trim(), "only regular files"),
    ] {
        if label == "symlink-tree" {
            git(&project, &["update-ref", EDIT_REF, commit, &base]);
        }
        // The host discards the session after a remote Act rejection.
        let mut host = f.open();
        let candidate = revision_check(&host, label, check, commit);
        let action = approve_for(&host, &candidate, "check.revision");
        let error = host.execute_approved(&adapter, &action).unwrap_err();
        assert!(
            matches!(
                error,
                arcs_host::HostError::Execution {
                    source: arcs::execution::ExecutionError::AdapterCall(
                        arcs::adapters::AdapterCallError::Rejected(ref detail)
                    ),
                    ..
                } if detail.contains(expected)
            ),
            "{label}: {error:?}"
        );
    }
    assert!(!f.0.join("revision-attempts").exists());
    assert_eq!(f.attempts(), 0);
}

#[test]
fn a_revision_receipt_replays_after_restart_even_when_the_ref_moves() {
    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    enable_revision_checks(&f);
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();
    let edit = edit_candidate(
        &host,
        "edit-before-verification",
        &base,
        "calculator.py",
        FIXED,
    );
    let action = approve_for(&host, &edit, "edit.apply");
    host.execute_approved(&adapter, &action).unwrap();
    let commit = revision(&project);
    let candidate = revision_check(&host, "repeat-receipt", "unit", &commit);
    let action = approve_for(&host, &candidate, "check.revision");
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    core.execute_batch(&format!(
        "CREATE TRIGGER lose_revision_receipt BEFORE INSERT ON artifact_versions
         WHEN NEW.schema_id='{REVISION_RECEIPT}' BEGIN SELECT RAISE(ABORT,'simulated Core receipt rollback'); END;"
    )).unwrap();
    assert!(host.execute_approved(&adapter, &action).is_err());
    let count_file = f.0.join("revision-attempts");
    assert_eq!(fs::read_to_string(&count_file).unwrap(), "unit\n");
    host.shutdown().unwrap();
    drop(host);
    core.execute_batch("DROP TRIGGER lose_revision_receipt")
        .unwrap();
    git(&project, &["update-ref", EDIT_REF, &base, &commit]);

    let mut host = f.open();
    let result = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(result.schema_id.0, REVISION_RECEIPT);
    assert_eq!(result.payload["result"]["status"], "passed");
    assert_eq!(result.payload["result"]["revision"], commit);
    assert_eq!(fs::read_to_string(&count_file).unwrap(), "unit\n");
    host.shutdown().unwrap();
}

const DIAGNOSTIC_CAPABILITY: &str = "check.revision.diagnostic";
const DIAGNOSTIC_RECEIPT: &str = "arcs.execution_result.workspace_revision_check.v2";

fn diagnostic_checks(f: &Fixture) -> String {
    let base = enable_edits(f, &f.0.join("project"));
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    let mut host = f.read("host.json");
    host["schemas"].as_array_mut().unwrap().extend([
        json!(schemas.join("revision-candidate.schema.json")),
        json!(schemas.join("revision-diagnostic-execution.schema.json")),
    ]);
    f.write("host.json", host);
    let mut grant = f.read("grant.json");
    grant["enabled_capabilities"] = json!([DIAGNOSTIC_CAPABILITY]);
    grant["granted_permissions"] = json!(["workspace.check_revision_diagnostics"]);
    f.write("grant.json", grant);
    let mut profile = f.read("profile.json");
    profile["diagnostic_revision_checks"] = json!(["loud", "quiet", "held-open", "timeout"]);
    profile["checks"]["loud"] = json!({
        "program":"/usr/bin/python3","timeout_ms":5000,"args":["-I","-c",r#"
import os, sys
from pathlib import Path
with Path(sys.argv[1]).open('a') as calls:
    calls.write('run\n')
os.write(1, b'stdout\n')
os.write(2, b'stderr\n\xff\x1b')
for _ in range(64):
    os.write(1, b'A' * 16384)
    os.write(2, b'B' * 16384)
raise SystemExit(1)
"#, f.0.join("diagnostic-attempts")]
    });
    profile["checks"]["quiet"] = json!({
        "program":"/usr/bin/python3","timeout_ms":3000,
        "args":["-I","-c","print('passing output is discarded')"]
    });
    profile["checks"]["held-open"] = json!({
        "program":"/usr/bin/python3","timeout_ms":5000,
        "args":["-I","-c",r#"
import subprocess, sys
subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(4)'])
print('failure with a background child', file=sys.stderr)
raise SystemExit(1)
"#]
    });
    profile["checks"]["timeout"] = json!({
        "program":"/usr/bin/python3","timeout_ms":500,
        "args":["-I","-u","-c","import time; print('started'); time.sleep(5)"]
    });
    f.write("profile.json", profile);
    base
}

fn diagnostic_candidate(host: &Host, check: &str, revision: &str) -> Artifact {
    propose(
        host,
        check,
        REVISION_SCHEMA,
        DIAGNOSTIC_CAPABILITY,
        json!({"check":check,"revision":revision}),
    )
}

#[test]
fn diagnostic_checks_bound_both_streams_and_replay_the_saved_failure_after_restart() {
    let f = Fixture::new();
    let base = diagnostic_checks(&f);
    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    let proposed = diagnostic_candidate(&host, "loud", &base);
    let attempts = f.0.join("diagnostic-attempts");
    assert!(
        host.execute_approved(&adapter, &proposed.version_id)
            .is_err()
    );
    assert!(!attempts.exists());
    let action = approve_for(&host, &proposed, DIAGNOSTIC_CAPABILITY);
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    core.execute_batch(&format!(
        "CREATE TRIGGER lose_diagnostic_receipt BEFORE INSERT ON artifact_versions
         WHEN NEW.schema_id='{DIAGNOSTIC_RECEIPT}' BEGIN SELECT RAISE(ABORT,'simulated Core receipt rollback'); END;"
    )).unwrap();
    assert!(host.execute_approved(&adapter, &action).is_err());
    assert_eq!(fs::read_to_string(&attempts).unwrap(), "run\n");
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let saved: String = journal
        .query_row("SELECT result FROM checks", [], |row| row.get(0))
        .unwrap();
    let saved: Value = serde_json::from_str(&saved).unwrap();
    host.shutdown().unwrap();
    drop(host);
    core.execute_batch("DROP TRIGGER lose_diagnostic_receipt")
        .unwrap();

    let mut host = f.open();
    let receipt = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(receipt.schema_id.0, DIAGNOSTIC_RECEIPT);
    assert_eq!(receipt.payload["action_version"], json!(action));
    assert_eq!(receipt.payload["result"]["revision"], base);
    assert_eq!(receipt.payload["result"]["status"], "failed");
    assert_eq!(receipt.payload["result"]["exit_code"], 1);
    assert_eq!(receipt.payload["diagnostics"], saved["diagnostics"]);
    let output = &receipt.payload["diagnostics"];
    for (stream, start) in [("stdout", "stdout\n"), ("stderr", "stderr\n�.")] {
        assert_eq!(output[stream]["truncated"], true);
        let captured = output[stream]["text"].as_str().unwrap();
        assert!(captured.starts_with(start), "{captured:?}");
        assert_eq!(captured.chars().count(), 512);
    }
    assert_eq!(fs::read_to_string(&attempts).unwrap(), "run\n");
    host.shutdown().unwrap();
}

#[test]
fn diagnostic_checks_preserve_deadlines_and_discard_successful_output() {
    let f = Fixture::new();
    let base = diagnostic_checks(&f);
    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    for (name, status) in [
        ("quiet", "passed"),
        ("held-open", "failed"),
        ("timeout", "timed_out_or_killed"),
    ] {
        let proposed = diagnostic_candidate(&host, name, &base);
        let action = approve_for(&host, &proposed, DIAGNOSTIC_CAPABILITY);
        let result = host.execute_approved(&adapter, &action).unwrap().payload;
        assert_eq!(result["result"]["status"], status);
        assert!(result["result"]["duration_ms"].as_u64().unwrap() < 3000);
        match name {
            "quiet" => assert!(result.get("diagnostics").is_none()),
            "held-open" => {
                assert_eq!(
                    result["diagnostics"]["stderr"]["text"],
                    "failure with a background child\n"
                );
                assert_eq!(result["diagnostics"]["stderr"]["truncated"], true);
            }
            "timeout" => assert_eq!(result["diagnostics"]["stdout"]["text"], "started\n"),
            _ => unreachable!(),
        }
    }
    host.shutdown().unwrap();
}

const SOURCE_REQUEST: &str = "arcs.workspace_source_request.v1";
const SOURCE_RESULT: &str = "arcs.workspace_source.v1";

fn configure_source(f: &Fixture, revision: &str, path: &str) {
    let mut host = f.read("host.json");
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    host["schemas"].as_array_mut().unwrap().extend([
        json!(schemas.join("source-request.schema.json")),
        json!(schemas.join("source.schema.json")),
    ]);
    host["adapters"][0]["requests"] = json!([{
        "id":"read-source","capability_id":"source.read",
        "input_schema_id":SOURCE_REQUEST,"response_schema_id":SOURCE_RESULT,
        "subject":"test-project","payload":{"revision":revision,"path":path}
    }]);
    f.write("host.json", host);
    f.write(
        "profile.json",
        json!({
            "adapter_id":ADAPTER,"workspace":"project","checks":{},
            "source":{"reference":EDIT_REF,"files":["calculator.py","link.py"]}
        }),
    );
    let mut grant = f.read("grant.json");
    grant["producer_class"] = json!("adapter");
    grant["ingress_source_kind"] = json!("file");
    grant["enabled_capabilities"] = json!(["source.read"]);
    grant["granted_permissions"] = json!(["workspace.source_read"]);
    f.write("grant.json", grant);
}

#[test]
fn source_request_recovers_the_exact_commit_bytes_after_the_ref_is_rewound() {
    let f = Fixture::new();
    let project = f.0.join("project");
    let base = enable_edits(&f, &project);
    fs::write(project.join("calculator.py"), FIXED).unwrap();
    git(&project, &["commit", "-qam", "Fixed revision"]);
    let commit = git(&project, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&project, &["update-ref", EDIT_REF, &commit, &base]);
    fs::write(project.join("calculator.py"), ORIGINAL).unwrap();
    configure_source(&f, &commit, "calculator.py");

    let mut host = f.open();
    let core = Connection::open(f.0.join("core.sqlite")).unwrap();
    core.execute_batch(&format!(
        "CREATE TRIGGER lose_source BEFORE INSERT ON artifact_versions
         WHEN NEW.schema_id='{SOURCE_RESULT}' BEGIN SELECT RAISE(ABORT,'lost source receipt'); END;"
    ))
    .unwrap();
    assert!(host.request("read-source").is_err());
    let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
    let payload: String = journal
        .query_row("SELECT payload FROM source_reads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        json!({
            "revision":commit,"path":"calculator.py","text":FIXED
        })
    );
    host.shutdown().unwrap();
    drop(host);
    core.execute_batch("DROP TRIGGER lose_source").unwrap();
    git(&project, &["update-ref", EDIT_REF, &base, &commit]);

    let mut host = f.open();
    let receipt = host.request("read-source").unwrap();
    assert_eq!(receipt.schema_id.0, SOURCE_RESULT);
    assert_eq!(
        receipt.payload,
        serde_json::from_str::<Value>(&payload).unwrap()
    );
    assert_eq!(revision(&project), base);
    assert_eq!(
        fs::read_to_string(project.join("calculator.py")).unwrap(),
        ORIGINAL
    );
    host.shutdown().unwrap();
}

#[test]
fn source_request_rejects_files_outside_its_repository_scope_or_byte_budget() {
    for case in ["other-path", "symlink", "unreachable", "response-limit"] {
        let f = Fixture::new();
        let project = f.0.join("project");
        std::os::unix::fs::symlink("calculator.py", project.join("link.py")).unwrap();
        let base = enable_edits(&f, &project);
        let (wanted, path, detail) = match case {
            "other-path" => (base.clone(), "untouched.txt", "path is not configured"),
            "symlink" => (base.clone(), "link.py", "requires a regular file"),
            "unreachable" => {
                git(
                    &project,
                    &["commit", "--allow-empty", "-qm", "outside the ARCS ref"],
                );
                (
                    git(&project, &["rev-parse", "HEAD"]).trim().to_owned(),
                    "calculator.py",
                    "reachable from the configured ref",
                )
            }
            "response-limit" => {
                fs::write(project.join("calculator.py"), "a".repeat(4096)).unwrap();
                git(
                    &project,
                    &["commit", "-qam", "File larger than the response budget"],
                );
                let commit = git(&project, &["rev-parse", "HEAD"]).trim().to_owned();
                git(&project, &["update-ref", EDIT_REF, &commit, &base]);
                (commit, "calculator.py", "response exceeds its byte limit")
            }
            _ => unreachable!(),
        };
        configure_source(&f, &wanted, path);
        let mut host = f.open();
        let error = host.request("read-source").unwrap_err().to_string();
        assert!(error.contains(detail), "{case}: {error}");
        let journal = Connection::open(f.0.join("checks.sqlite")).unwrap();
        assert_eq!(
            journal
                .query_row("SELECT count(*) FROM source_reads", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

const ISOLATED_CAPABILITY: &str = "check.revision.isolated";

fn configure_isolated_checks(f: &Fixture, program: &Path, definitions: Value) -> String {
    let base = enable_edits(f, &f.0.join("project"));
    let schemas = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    let mut host = f.read("host.json");
    host["adapters"][0]["inherit_env"] = json!(["ARCS_TEST_BWRAP"]);
    host["schemas"].as_array_mut().unwrap().extend([
        json!(schemas.join("revision-candidate.schema.json")),
        json!(schemas.join("revision-diagnostic-execution.schema.json")),
    ]);
    f.write("host.json", host);
    let mut grant = f.read("grant.json");
    grant["enabled_capabilities"] = json!(["check.run", ISOLATED_CAPABILITY]);
    grant["granted_permissions"] = json!(["workspace.check", "workspace.check_revision_isolated"]);
    f.write("grant.json", grant);

    let mut profile = f.read("profile.json");
    let definitions = definitions.as_object().unwrap();
    profile["checks"]
        .as_object_mut()
        .unwrap()
        .extend(definitions.clone());
    profile["isolated_revision_checks"] =
        json!({"program":program,"checks":definitions.keys().collect::<Vec<_>>()});
    f.write("profile.json", profile);
    base
}

fn isolated_candidate(host: &Host, name: &str, revision: &str) -> Artifact {
    propose(
        host,
        name,
        REVISION_SCHEMA,
        ISOLATED_CAPABILITY,
        json!({"check":name,"revision":revision}),
    )
}

#[test]
fn a_missing_launcher_receipt_cannot_pass_or_fall_back_to_an_unisolated_check() {
    let f = Fixture::new();
    let marker = f.0.join("unsandboxed");
    // A launcher that returns success without having started a sandbox.
    let base = configure_isolated_checks(
        &f,
        Path::new("/usr/bin/true"),
        json!({
            "must-isolate":{"program":"/usr/bin/python3","timeout_ms":2000,
                "args":["-I","-c","import pathlib,sys; pathlib.Path(sys.argv[1]).touch()",marker]}
        }),
    );

    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    let proposed = isolated_candidate(&host, "must-isolate", &base);
    let action = approve_for(&host, &proposed, ISOLATED_CAPABILITY);
    let receipt = host.execute_approved(&adapter, &action).unwrap();
    assert_eq!(receipt.payload["result"]["status"], "runner_error");
    assert!(!marker.exists());
    let bypass = approve(&host, &candidate(&host, "legacy-bypass", "must-isolate"));
    let error = host
        .execute_approved(&adapter, &bypass)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("isolated check requires a Git revision"),
        "{error}"
    );
    assert!(!marker.exists());
}

#[test]
#[ignore = "requires Bubblewrap and usable Linux user and network namespaces"]
fn bubblewrap_isolates_the_revision_and_reports_workload_exit_and_timeout() {
    let program = std::env::var_os("ARCS_TEST_BWRAP")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/bwrap"));
    let f = Fixture::new();
    let host_file = f.0.join("host-only");
    fs::write(&host_file, "outside the sandbox").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let host_port = listener.local_addr().unwrap().port();
    let host_pid = std::process::id();
    let probe = r#"
import os
from pathlib import Path
import socket
import sys
import time
assert Path.cwd() == Path('/workspace')
assert Path('calculator.py').read_text().startswith('def add')
assert not Path(sys.argv[1]).exists()
assert os.environ.get('HOME') == '/tmp' and 'ARCS_TEST_BWRAP' not in os.environ
assert not Path('/proc', sys.argv[3]).exists()
assert int(next(line.split()[1] for line in Path('/proc/self/status').read_text().splitlines()
                if line.startswith('CapEff:')), 16) == 0
try:
    Path('changed').write_text('no')
except OSError as error:
    import errno
    assert error.errno == errno.EROFS
else:
    raise AssertionError('workspace was writable')
Path('/tmp/scratch').write_text('yes')
try:
    with socket.create_connection(('127.0.0.1', int(sys.argv[2])), .2):
        raise AssertionError('host listener was reachable')
except OSError:
    pass
if sys.argv[4] == 'fail':
    print('isolated check failed', file=sys.stderr, flush=True)
    raise SystemExit(17)
if sys.argv[4] == 'slow':
    print('isolated check started', flush=True)
    time.sleep(10)
"#;
    let definition = |mode, duration| {
        json!({
            "program":"/usr/bin/python3","timeout_ms":duration,
            "args":["-I","-B","-c",probe,host_file,host_port.to_string(),host_pid.to_string(),mode]
        })
    };
    let base = configure_isolated_checks(
        &f,
        &program,
        json!({
            "scope":definition("pass", 5000),
            "failed":definition("fail", 5000),
            "slow-isolated":definition("slow", 1000),
        }),
    );
    let mut host = f.open();
    let adapter = AdapterId(ADAPTER.into());
    for (name, status) in [
        ("scope", "passed"),
        ("failed", "failed"),
        ("slow-isolated", "timed_out_or_killed"),
    ] {
        let proposed = isolated_candidate(&host, name, &base);
        let action = approve_for(&host, &proposed, ISOLATED_CAPABILITY);
        let payload = host.execute_approved(&adapter, &action).unwrap().payload;
        assert_eq!(payload["result"]["status"], status, "{payload:?}");
        assert_eq!(payload["result"]["revision"], base);
        if name == "failed" {
            assert_eq!(payload["result"]["exit_code"], 17);
            assert_eq!(
                payload["diagnostics"]["stderr"]["text"],
                "isolated check failed\n"
            );
        }
        if name == "slow-isolated" {
            assert_eq!(
                payload["diagnostics"]["stdout"]["text"],
                "isolated check started\n"
            );
        }
    }
    assert!(!f.0.join("project/changed").exists());
    assert_eq!(
        fs::read_to_string(&host_file).unwrap(),
        "outside the sandbox"
    );
    host.shutdown().unwrap();
}

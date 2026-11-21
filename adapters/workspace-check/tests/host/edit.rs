use super::*;
use std::path::Path;
use std::process::Command;

const EDIT_REF: &str = "refs/arcs/test";
const EDIT_SCHEMA: &str = "arcs.workspace_edit_candidate.v1";
const EDIT_RECEIPT: &str = "arcs.execution_result.workspace_edit.v1";
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
    #[cfg(unix)]
    std::os::unix::fs::symlink("calculator.py", project.join("link.py")).unwrap();
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
    profile["checks"]["unit"] = json!({"program":"/usr/bin/python3","args":["-I","-c",r#"
import pathlib, subprocess, sys, tempfile
revision = subprocess.check_output(['/usr/bin/git','rev-parse','refs/arcs/test'], text=True).strip()
blob = subprocess.check_output(['/usr/bin/git','show',revision+':calculator.py'])
with tempfile.TemporaryDirectory() as name:
    pathlib.Path(name,'calculator.py').write_bytes(blob)
    result = subprocess.run([sys.executable,'-B','-c','from calculator import add; assert add(2,3)==5'],cwd=name)
    sys.exit(result.returncode)
"#],"timeout_ms":3000});
    f.write("profile.json", profile);
    let mut grant = f.read("grant.json");
    grant["enabled_capabilities"] = json!(["check.run", "edit.apply"]);
    grant["granted_permissions"] = json!(["workspace.check", "workspace.edit"]);
    f.write("grant.json", grant);
    base
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
    let base = enable_edits(&f, &project);
    let adapter = AdapterId(ADAPTER.into());
    let mut host = f.open();
    let before = approve(&host, &candidate(&host, "unit-before", "unit"));
    assert_eq!(
        host.execute_approved(&adapter, &before).unwrap().payload["result"]["status"],
        "failed"
    );

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
    let verified = approve(&host, &candidate(&host, "unit-after", "unit"));
    assert_eq!(
        host.execute_approved(&adapter, &verified).unwrap().payload["result"]["status"],
        "passed"
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

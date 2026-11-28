//! Nachweis der formalen Nachweise in der Zielruntime.
//!
//! Der Test fährt einen echten Zielablauf: Ein vorgegebenes fan-Ziel wartet
//! auf einen formalen Nachweis, bevor es wirken darf. Er prüft die Warte-,
//! Freigabe- und Blockadefälle sowie die Wiederaufnahme nach einem Neustart.

mod support;

use arcs::core::VersionId;
use arcs::formal::FormalEvidenceService;
use arcs_host::Host;
use arcs_host::task_runtime::TaskRuntime;
use arcs_host::task_runtime::formal::{FormalDefinitionInput, FormalObligationRequest};
use arcs_host::task_runtime::goal::{GoalState, GoalTask};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Die Obligation, die das Ziel nachweisen muss.
const OBLIGATION: &str = "fan-cycle-safety";

/// Ein belegbarer Satz: Lean muss ihn ohne Zusatzannahmen beweisen können.
const TRUE_STATEMENT: &str = "(2:Nat) + 2 = 4";

/// Ein falscher Satz: Lean widerlegt ihn, er trägt die Handlung nicht.
const FALSE_STATEMENT: &str = "(2:Nat) + 2 = 5";

/// Die Grundlage der Lean-Werkzeugkette.
///
/// Der Core kennt die Axiome des Werkzeugs nicht. Die Obligation nennt
/// daher ausdrücklich, welche Axiome ein Nachweis verwenden darf.
const LEAN_BASIS: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

struct Fixture {
    directory: PathBuf,
    lean: PathBuf,
}

impl Fixture {
    /// Baut die Umgebung mit einer bestimmten Lean-Werkzeugkette.
    ///
    /// Ein Pfad, den es nicht gibt, bildet den Fall ab, in dem das formale
    /// Werkzeug nicht verfügbar ist.
    fn new(lean: PathBuf) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-formal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/formal");
        for file in std::fs::read_dir(&source).unwrap() {
            let file = file.unwrap();
            std::fs::copy(file.path(), directory.join(file.file_name())).unwrap();
        }
        let fixture = Self { directory, lean };
        let mut host = fixture.read("host.json");
        host["database"] = json!(fixture.directory.join("core.sqlite"));
        for schema in host["schemas"].as_array_mut().unwrap() {
            *schema = json!(
                source
                    .join(schema.as_str().unwrap())
                    .canonicalize()
                    .unwrap()
            );
        }
        for adapter in host["adapters"].as_array_mut().unwrap() {
            let name = PathBuf::from(adapter["program"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            adapter["program"] = json!(support::adapter_binary(&name));
            if name == "arcs-formal-check" {
                // Das formale Werkzeug bekommt seine Werkzeugkette aus dem
                // Aufruf, damit der Fall „nicht verfügbar" prüfbar bleibt.
                adapter["args"] = json!(["--lean-binary", fixture.lean.to_str().unwrap()]);
            } else {
                let profile = adapter["profile"].as_str().unwrap();
                adapter["args"][0] = json!({
                    "path": fixture.directory.join(profile.replace("-profile.json", ".sqlite"))
                });
            }
        }
        fixture.write("host.json", host);
        // Die Grenzen der Reasoner-Grant müssen zum Zeitbudget der Route
        // passen. Sonst verweigert der Host die Route schon beim Start.
        for role in ["planner", "assessor"] {
            let file = format!("{role}-grant.json");
            let mut grant = fixture.read(&file);
            grant["reasoning_limits_v2"]["resources"]["max_runtime_ms"] = json!(15000);
            fixture.write(&file, grant);
        }
        let mut runtime = fixture.read("runtime.json");
        runtime["journal"] = json!(fixture.directory.join("runtime.sqlite"));
        runtime["interval_ms"] = json!(1);
        runtime["max_jobs_per_tick"] = json!(1);
        for route in runtime["routes"].as_array_mut().unwrap() {
            route["budget"]["max_runtime_ms"] = json!(15000);
            route["goal"]["assessment"]["budget"]["max_runtime_ms"] = json!(15000);
        }
        fixture.write("runtime.json", runtime);
        fixture
    }

    fn read(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.directory.join(name)).unwrap()).unwrap()
    }
    fn write(&self, name: &str, value: Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    fn host(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn runtime(&self, host: &Host) -> TaskRuntime {
        TaskRuntime::open(
            host,
            arcs_host::task_runtime::RuntimeConfig::load(self.directory.join("runtime.json"))
                .unwrap(),
        )
        .unwrap()
    }
    /// Legt die Obligation an, die das Ziel nachweisen muss.
    fn obligate(
        &self,
        runtime: &mut TaskRuntime,
        host: &Host,
        statement: &str,
        goal: Option<&str>,
    ) -> VersionId {
        runtime
            .register_formal_obligation(
                host,
                &FormalObligationRequest {
                    name: OBLIGATION.to_owned(),
                    statement: statement.to_owned(),
                    definitions: vec![FormalDefinitionInput {
                        name: "fan_is_safe".into(),
                        body: "def fan_is_safe : Prop := True".into(),
                    }],
                    assumptions: Vec::new(),
                    goal: goal.map(|goal| goal.to_owned()),
                    plan_revision: None,
                    allowed_axioms: LEAN_BASIS.iter().map(|axiom| (*axiom).to_owned()).collect(),
                },
            )
            .unwrap()
    }
    /// Zählt die Schaltvorgänge des simulierten Lüfters.
    fn actuations(&self) -> i64 {
        rusqlite::Connection::open(self.directory.join("fan.sqlite"))
            .unwrap()
            .query_row("SELECT actuation_count FROM fan_state", [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn lean_binary() -> Option<PathBuf> {
    let path = PathBuf::from("/home/simon/.elan/bin/lean");
    if path.exists() {
        return Some(path);
    }
    let output = std::process::Command::new("lean")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    output.success().then(|| PathBuf::from("lean"))
}

/// Fährt den Zielablauf, bis der erwartete Zustand erreicht ist.
fn until(
    host: &mut Host,
    runtime: &mut TaskRuntime,
    expected: impl Fn(&GoalTask) -> bool,
) -> GoalTask {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        runtime.tick(host).unwrap();
        let current = runtime.goals_after(0, 1).unwrap().into_iter().next();
        if let Some(task) = current.as_ref().filter(|task| expected(task)) {
            return task.clone();
        }
        assert!(
            Instant::now() < deadline,
            "the goal did not reach the expected state: {current:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Wartet auf den Wartezustand, der vom formalen Nachweis stammt.
///
/// Der Zielablauf kennt auch ein gewöhnliches Prüfen. Der formale
/// Wartezustand ist daran erkennbar, dass er den fehlenden Nachweis nennt.
fn wait_for_formal_wait(host: &mut Host, runtime: &mut TaskRuntime) -> GoalTask {
    until(host, runtime, |task| {
        task.state == GoalState::Verifying && task.detail.contains("formal")
    })
}

/// Liest den formalen Bericht der Obligation aus dem Core-Store.
fn report(host: &Host, name: &str) -> Option<arcs::core::Artifact> {
    host.store()
        .current(
            &arcs::core::SubjectId(format!("formal:report:{name}")),
            &arcs::core::SchemaId("arcs.formal_verification_report.v1".into()),
        )
        .unwrap()
}

#[test]
fn a_goal_acts_only_after_the_prescribed_obligation_is_proven() {
    let Some(lean) = lean_binary() else {
        eprintln!("no Lean toolchain available");
        return;
    };
    let fixture = Fixture::new(lean);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    fixture.obligate(&mut runtime, &host, TRUE_STATEMENT, Some("fan-cycle"));

    let task = until(&mut host, &mut runtime, |task| {
        matches!(
            task.state,
            GoalState::Completed | GoalState::Blocked | GoalState::Failed
        )
    });
    assert_eq!(task.state, GoalState::Completed, "{task:?}");
    // Die Handlung ist erst nach dem Nachweis erfolgt.
    assert_eq!(fixture.actuations(), 2);

    let report = report(&host, OBLIGATION).expect("the formal report must exist");
    assert_eq!(report.payload["verdict"], "proved");
    assert_eq!(report.payload["statement"], TRUE_STATEMENT);
    assert!(
        report.payload["proof_version"].is_string(),
        "the report must name a verified proof: {report:?}"
    );
    host.shutdown().unwrap();
}

#[test]
fn a_missing_formal_toolchain_keeps_the_goal_waiting_without_acting() {
    let fixture = Fixture::new(PathBuf::from("/nonexistent/arcs-lean-binary"));
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    fixture.obligate(&mut runtime, &host, TRUE_STATEMENT, Some("fan-cycle"));

    let task = wait_for_formal_wait(&mut host, &mut runtime);
    assert_eq!(task.state, GoalState::Verifying);
    assert!(
        task.detail.contains("formal"),
        "the reason must name the missing evidence: {task:?}"
    );
    // Der Lüfter wurde nicht betätigt: Ohne Nachweis wirkt die Handlung nicht.
    assert_eq!(fixture.actuations(), 0);
    host.shutdown().unwrap();
}

#[test]
fn a_disproved_obligation_blocks_the_goal() {
    let Some(lean) = lean_binary() else {
        eprintln!("no Lean toolchain available");
        return;
    };
    let fixture = Fixture::new(lean);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    fixture.obligate(&mut runtime, &host, FALSE_STATEMENT, Some("fan-cycle"));

    let task = until(&mut host, &mut runtime, |task| {
        matches!(
            task.state,
            GoalState::Blocked | GoalState::Completed | GoalState::Failed
        )
    });
    assert_eq!(task.state, GoalState::Blocked, "{task:?}");
    assert_eq!(fixture.actuations(), 0);

    let report = report(&host, OBLIGATION).expect("the formal report must exist");
    assert_eq!(report.payload["verdict"], "disproved");
    host.shutdown().unwrap();
}

#[test]
fn an_obligation_of_another_goal_does_not_prove_this_one() {
    let Some(lean) = lean_binary() else {
        eprintln!("no Lean toolchain available");
        return;
    };
    let fixture = Fixture::new(lean);
    let mut host = fixture.host();
    let mut runtime = fixture.runtime(&host);
    // Die Obligation gehört einem anderen Ziel und trägt dieses Ziel nicht.
    fixture.obligate(&mut runtime, &host, TRUE_STATEMENT, Some("other-goal"));

    let task = until(&mut host, &mut runtime, |task| {
        matches!(
            task.state,
            GoalState::Blocked | GoalState::Completed | GoalState::Failed
        )
    });
    assert_eq!(task.state, GoalState::Blocked, "{task:?}");
    assert_eq!(fixture.actuations(), 0);
    host.shutdown().unwrap();
}

#[test]
fn a_proven_obligation_survives_a_restart_without_a_new_check() {
    let Some(lean) = lean_binary() else {
        eprintln!("no Lean toolchain available");
        return;
    };
    let fixture = Fixture::new(lean);
    let before = {
        let mut host = fixture.host();
        let mut runtime = fixture.runtime(&host);
        fixture.obligate(&mut runtime, &host, TRUE_STATEMENT, Some("fan-cycle"));
        let task = until(&mut host, &mut runtime, |task| {
            matches!(
                task.state,
                GoalState::Completed | GoalState::Blocked | GoalState::Failed
            )
        });
        assert_eq!(task.state, GoalState::Completed, "{task:?}");
        let report = report(&host, OBLIGATION).expect("the formal report must exist");
        host.shutdown().unwrap();
        report.version_id
    };

    // Nach dem Neustart liegt der Nachweis vor. Er wird nicht neu erzeugt.
    let mut host = fixture.host();
    let report_after = report(&host, OBLIGATION).expect("the report must survive");
    assert_eq!(report_after.version_id, before);
    assert_eq!(report_after.payload["verdict"], "proved");

    let mut ids = arcs::core::SequenceIdGenerator::new("formal-test");
    let decisions = FormalEvidenceService::new(
        host.schemas(),
        host.store(),
        &mut ids,
        &arcs::core::SystemClock,
    );
    let obligation = host
        .store()
        .current(
            &arcs::core::SubjectId(format!("formal:obligation:{OBLIGATION}")),
            &arcs::core::SchemaId("arcs.proof_obligation.v1".into()),
        )
        .unwrap()
        .unwrap();
    let decision = decisions
        .decide(
            &obligation.version_id,
            &arcs::formal::EvidenceBinding::plan(None, Some("fan-cycle".into())),
        )
        .unwrap();
    assert_eq!(
        decision,
        arcs::formal::EvidenceDecision::Allowed,
        "the stored evidence must still carry the action"
    );
    host.shutdown().unwrap();
}

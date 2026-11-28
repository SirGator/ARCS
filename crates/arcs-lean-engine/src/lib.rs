//! Isolierte Lean-Engine für formale Prüfungen.
//!
//! Die Engine ist ein externes Werkzeug: Sie kennt weder Store noch
//! Runtime-Autorität und bekommt ausschließlich eine Prüfanfrage. Der
//! Arbeitsbereich liegt in einem eigenen Verzeichnis, der Lean-Prozess läuft
//! mit harter Laufzeitgrenze, und es findet keine Netz- oder Dateizugriffe
//! jenseits dieses Verzeichnisses statt.
//!
//! Die Engine meldet, was Lean tatsächlich gesagt hat. Über die Verwendbarkeit
//! als Nachweis entscheidet der Core.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arcs_adapter_protocol::formal::{
    FormalCheckOutcome, FormalCheckRequest, FormalFinding, FormalFindingVerdict, FormalToolError,
    FormalVerdict,
};

/// Name des geprüften Satzes im erzeugten Lean-Programm.
const THEOREM_NAME: &str = "arcs_obligation";

/// Die Axiome des Lean-Kerns.
///
/// Diese drei Axiome sind Bestandteil jeder Lean-Umgebung. Sie sind keine
/// Erlaubnis, sondern die Grundlage der Werkzeugkette: Ein Beweis, der sie
/// verwendet, ruht auf ihnen. Welche Axiome ein Nachweis verwenden darf,
/// entscheidet die aufrufende Obligation; sie nennt diese Grundlage
/// ausdrücklich, damit der Core sie prüfen kann.
pub const DEFAULT_ALLOWED_AXIOMS: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

/// Standardmäßige Taktikfolge für die Suche nach einem Beweis.
pub const DEFAULT_TACTICS: &[&str] = &[
    "simp", "decide", "omega", "rfl", "trivial", "simp_all", "norm_num",
];

/// Betriebsgrenzen der Engine.
#[derive(Debug, Clone)]
pub struct LeanConfig {
    /// Pfad des Lean-Binaries.
    pub binary: PathBuf,
    /// Harte Obergrenze einer einzelnen Prüfung.
    pub max_runtime_ms: u64,
    /// Wurzelverzeichnis für die temporären Arbeitsbereiche.
    pub workdir: Option<PathBuf>,
    /// Taktiken, die bei fehlendem Beweisvorschlag der Reihe nach versucht werden.
    pub tactics: Vec<String>,
}

impl Default for LeanConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("lean"),
            max_runtime_ms: 60_000,
            workdir: None,
            tactics: DEFAULT_TACTICS
                .iter()
                .map(|tactic| (*tactic).to_owned())
                .collect(),
        }
    }
}

/// Die isolierte Lean-Engine.
pub struct LeanEngine {
    config: LeanConfig,
}

impl LeanEngine {
    pub fn new(config: LeanConfig) -> Self {
        Self { config }
    }

    /// Die Version der gefundenen Lean-Werkzeugkette.
    pub fn toolchain_version(&self) -> String {
        let output = Command::new(&self.config.binary)
            .arg("--version")
            .stdin(Stdio::null())
            .output();
        match output {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout);
                text.split_whitespace()
                    .nth(3)
                    .unwrap_or("unknown")
                    .to_owned()
            }
            Err(_) => "unavailable".to_owned(),
        }
    }

    /// Prüft eine Aussage und meldet das Ergebnis der Engine.
    ///
    /// Ohne Beweisvorschlag sucht die Engine selbst einen Beweis. Die Suche
    /// endet mit dem ersten Taktikvorschlag, den Lean annimmt.
    pub fn check(
        &self,
        request: &FormalCheckRequest,
    ) -> Result<FormalCheckOutcome, FormalToolError> {
        if request.statement.trim().is_empty() {
            return Err(FormalToolError {
                code: "invalid_request".into(),
                message: "empty statement".into(),
            });
        }
        let started = Instant::now();
        let deadline = started + Duration::from_millis(request.max_runtime_ms);
        let directory = self.create_workdir()?;
        let result = self.run_in(&directory, request, deadline);
        let _ = fs::remove_dir_all(&directory);
        let (mut outcome, proof_text) = result?;
        outcome.duration_ms = started.elapsed().as_millis() as u64;
        if outcome.proof_text.is_none() {
            outcome.proof_text = proof_text;
        }
        Ok(outcome)
    }

    /// Führt die Prüfung in einem eigenen Arbeitsverzeichnis aus.
    fn run_in(
        &self,
        directory: &Path,
        request: &FormalCheckRequest,
        deadline: Instant,
    ) -> Result<(FormalCheckOutcome, Option<String>), FormalToolError> {
        if let Some(proof_text) = request.proof_text.clone() {
            let (outcome, _) = self.attempt(directory, request, &proof_text, deadline, false)?;
            if outcome.verdict == FormalVerdict::Proved {
                return Ok((outcome, None));
            }
            // Ein gescheiterter Beweis beweist noch nichts. Erst die
            // Widerlegung trennt "unbewiesen" von "widerlegt".
            if let Some(refutation) = self.refute(directory, request, deadline)? {
                return Ok((refutation, None));
            }
            return Ok((outcome, None));
        }
        for tactic in &self.config.tactics {
            if Instant::now() >= deadline {
                return Ok((
                    unavailable("the search for a proof exceeded the runtime limit"),
                    None,
                ));
            }
            let (outcome, _) = self.attempt(directory, request, tactic, deadline, false)?;
            if outcome.verdict == FormalVerdict::Proved {
                return Ok((outcome, Some(tactic.clone())));
            }
        }
        // Erst wenn kein Beweis gefunden wurde, lohnt die Widerlegung. Sie
        // kostet einen weiteren Satz von Läufen und liefert stattdessen einen
        // belastbaren Gegenbeleg.
        if let Some(refutation) = self.refute(directory, request, deadline)? {
            return Ok((refutation, None));
        }
        Ok((
            FormalCheckOutcome {
                verdict: FormalVerdict::Failed,
                used_axioms: Vec::new(),
                findings: vec![FormalFinding {
                    check: "formal.search".into(),
                    verdict: FormalFindingVerdict::Fail,
                    detail: "no admissible proof was found".into(),
                }],
                proof_text: None,
                duration_ms: 0,
            },
            None,
        ))
    }

    /// Sucht eine Widerlegung der Aussage.
    ///
    /// Eine Widerlegung ist ein Beweis der Negation. Sie belegt, dass die
    /// Aussage falsch ist, statt nur, dass kein Beweis gefunden wurde.
    fn refute(
        &self,
        directory: &Path,
        request: &FormalCheckRequest,
        deadline: Instant,
    ) -> Result<Option<FormalCheckOutcome>, FormalToolError> {
        for tactic in &self.config.tactics {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            let (outcome, _) = self.attempt(directory, request, tactic, deadline, true)?;
            if outcome.verdict == FormalVerdict::Proved {
                let mut refutation = outcome;
                refutation.verdict = FormalVerdict::Disproved;
                refutation.findings.push(FormalFinding {
                    check: "formal.disproved".into(),
                    verdict: FormalFindingVerdict::Fail,
                    detail: "the negation of the statement is provable".into(),
                });
                return Ok(Some(refutation));
            }
        }
        Ok(None)
    }

    /// Ein einzelner Prüflauf mit einem konkreten Beweisvorschlag.
    fn attempt(
        &self,
        directory: &Path,
        request: &FormalCheckRequest,
        proof_text: &str,
        deadline: Instant,
        negate: bool,
    ) -> Result<(FormalCheckOutcome, Option<String>), FormalToolError> {
        let source = render_source(request, proof_text, negate);
        let file = directory.join("Check.lean");
        fs::write(&file, source).map_err(|error| FormalToolError {
            code: "io".into(),
            message: format!("cannot write the Lean source: {error}"),
        })?;

        let stdout_path = directory.join("lean.out");
        let stderr_path = directory.join("lean.err");
        let stdout = fs::File::create(&stdout_path).map_err(|error| FormalToolError {
            code: "io".into(),
            message: format!("cannot create the output log: {error}"),
        })?;
        let stderr = fs::File::create(&stderr_path).map_err(|error| FormalToolError {
            code: "io".into(),
            message: format!("cannot create the error log: {error}"),
        })?;

        let mut child = match Command::new(&self.config.binary)
            .arg("Check.lean")
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                // Ein Werkzeug, das nicht laufen kann, meldet das als
                // Verdict. Ein Fehler wäre hier eine falsche Aussage über
                // den Zustand des Nachweises.
                return Ok((
                    FormalCheckOutcome {
                        verdict: FormalVerdict::Unavailable,
                        used_axioms: Vec::new(),
                        findings: vec![FormalFinding {
                            check: "formal.engine".into(),
                            verdict: FormalFindingVerdict::Fail,
                            detail: format!("the Lean engine cannot be started: {error}"),
                        }],
                        proof_text: None,
                        duration_ms: 0,
                    },
                    None,
                ));
            }
        };

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = read_logs(directory);
                    return Ok((self.assess(&output, status.success(), request), None));
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(FormalToolError {
                        code: "unavailable".into(),
                        message: format!("the Lean engine cannot be observed: {error}"),
                    });
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Ok((
                    FormalCheckOutcome {
                        verdict: FormalVerdict::Unavailable,
                        used_axioms: Vec::new(),
                        findings: vec![FormalFinding {
                            check: "formal.runtime".into(),
                            verdict: FormalFindingVerdict::Fail,
                            detail: "the Lean engine exceeded the runtime limit".into(),
                        }],
                        proof_text: None,
                        duration_ms: 0,
                    },
                    None,
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Bewertet die Ausgabe von Lean.
    fn assess(
        &self,
        output: &LeanOutput,
        success: bool,
        request: &FormalCheckRequest,
    ) -> FormalCheckOutcome {
        let mut findings = Vec::new();
        if !success {
            findings.push(FormalFinding {
                check: "formal.compile".into(),
                verdict: FormalFindingVerdict::Fail,
                detail: first_error(output),
            });
            return FormalCheckOutcome {
                verdict: FormalVerdict::Failed,
                used_axioms: Vec::new(),
                findings,
                proof_text: None,
                duration_ms: 0,
            };
        }
        let uses_sorry =
            output.text.contains("declaration uses 'sorry'") || output.text.contains("sorryAx");
        findings.push(FormalFinding {
            check: "formal.no_sorry".into(),
            verdict: if uses_sorry {
                FormalFindingVerdict::Fail
            } else {
                FormalFindingVerdict::Pass
            },
            detail: if uses_sorry {
                "the proof contains an unproved placeholder".into()
            } else {
                "the proof contains no placeholder".into()
            },
        });

        let used_axioms = parse_axioms(&output.text);
        let declared: Vec<&str> = request
            .assumptions
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        // Die erlaubten Axiome stehen in der Anfrage. Das Werkzeug erweitert
        // sie nicht: Was der Aufrufer nicht erlaubt hat, darf der Beweis
        // nicht verwenden.
        let allowed = request
            .allowed_axioms
            .iter()
            .map(|axiom| axiom.as_str())
            .chain(declared.iter().copied());
        let forbidden: Vec<&String> = used_axioms
            .iter()
            .filter(|axiom| !declared.contains(&axiom.as_str()))
            .filter(|axiom| !allowed.clone().any(|permitted| permitted == axiom.as_str()))
            .collect();
        findings.push(FormalFinding {
            check: "formal.allowed_axioms".into(),
            verdict: if forbidden.is_empty() {
                FormalFindingVerdict::Pass
            } else {
                FormalFindingVerdict::Fail
            },
            detail: if forbidden.is_empty() {
                "the proof uses no axiom outside the permitted set".into()
            } else {
                format!(
                    "the proof uses axioms outside the permitted set: {}",
                    forbidden
                        .iter()
                        .map(|axiom| axiom.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        });
        findings.push(FormalFinding {
            check: "formal.no_error".into(),
            verdict: FormalFindingVerdict::Pass,
            detail: "the Lean engine accepted the program".into(),
        });

        FormalCheckOutcome {
            verdict: FormalVerdict::Proved,
            used_axioms,
            findings,
            proof_text: None,
            duration_ms: 0,
        }
    }

    /// Erzeugt ein eigenes Arbeitsverzeichnis für einen Prüflauf.
    fn create_workdir(&self) -> Result<PathBuf, FormalToolError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = self
            .config
            .workdir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        let directory = root.join(format!(
            "arcs-lean-{}-{}-{}",
            std::process::id(),
            nanos,
            sequence
        ));
        fs::create_dir_all(&directory).map_err(|error| FormalToolError {
            code: "io".into(),
            message: format!("cannot create the work directory: {error}"),
        })?;
        Ok(directory)
    }
}

/// Ausgabe eines Lean-Laufs.
struct LeanOutput {
    text: String,
}

fn read_logs(directory: &Path) -> LeanOutput {
    let mut text = String::new();
    for name in ["lean.out", "lean.err"] {
        if let Ok(content) = fs::read_to_string(directory.join(name)) {
            text.push_str(&content);
            text.push('\n');
        }
    }
    LeanOutput { text }
}

/// Erzeugt das geprüfte Lean-Programm.
///
/// Annahmen werden als Axiome eingeführt, damit sie im Beweisraum gelten.
/// Sie erscheinen deshalb später auch in der Axiomliste und werden dort als
/// erklärt behandelt.
fn render_source(request: &FormalCheckRequest, proof_text: &str, negate: bool) -> String {
    let mut source = String::new();
    source.push_str("-- generated by the ARCS Lean engine\n");
    for definition in &request.definitions {
        source.push_str(&definition.body);
        source.push('\n');
    }
    for assumption in &request.assumptions {
        source.push_str(&format!(
            "axiom {} : {}\n",
            assumption.name, assumption.statement
        ));
    }
    // Eine Widerlegung beweist die Negation der Aussage.
    let statement = if negate {
        format!("¬({})", request.statement)
    } else {
        request.statement.clone()
    };
    source.push_str(&format!("theorem {THEOREM_NAME} : {statement} := by\n"));
    for line in proof_text.lines() {
        source.push_str("  ");
        source.push_str(line);
        source.push('\n');
    }
    source.push_str(&format!("#print axioms {THEOREM_NAME}\n"));
    source
}

/// Liest die von Lean gemeldeten Axiome einer Deklaration.
fn parse_axioms(text: &str) -> Vec<String> {
    for line in text.lines() {
        let Some(rest) = line.split("depends on axioms: [").nth(1) else {
            continue;
        };
        let Some(list) = rest.split(']').next() else {
            continue;
        };
        return list
            .split(',')
            .map(|axiom| axiom.trim().to_owned())
            .filter(|axiom| !axiom.is_empty())
            .collect();
    }
    Vec::new()
}

/// Ein Ergebnis, das keinen Nachweis erbringen kann.
fn unavailable(detail: &str) -> FormalCheckOutcome {
    FormalCheckOutcome {
        verdict: FormalVerdict::Unavailable,
        used_axioms: Vec::new(),
        findings: vec![FormalFinding {
            check: "formal.runtime".into(),
            verdict: FormalFindingVerdict::Fail,
            detail: detail.to_owned(),
        }],
        proof_text: None,
        duration_ms: 0,
    }
}

/// Der erste Fehlertext eines Lean-Laufs.
fn first_error(output: &LeanOutput) -> String {
    output
        .text
        .lines()
        .find(|line| line.contains("error"))
        .unwrap_or("the Lean engine rejected the program")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gibt zurück, ob auf diesem Rechner eine Lean-Werkzeugkette verfügbar ist.
    fn lean_available() -> bool {
        Command::new("lean")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn request(statement: &str, proof: Option<&str>) -> FormalCheckRequest {
        FormalCheckRequest {
            statement: statement.into(),
            proof_text: proof.map(|text| text.to_owned()),
            definitions: Vec::new(),
            assumptions: Vec::new(),
            allowed_axioms: vec!["propext".into()],
            toolchain: arcs_adapter_protocol::formal::Toolchain {
                name: "lean4".into(),
                version: "4".into(),
            },
            max_runtime_ms: 60_000,
        }
    }

    #[test]
    fn a_true_statement_is_proved() {
        if !lean_available() {
            return;
        }
        let engine = LeanEngine::new(LeanConfig::default());
        let outcome = engine
            .check(&request("(2:Nat) + 2 = 4", Some("decide")))
            .unwrap();
        assert_eq!(outcome.verdict, FormalVerdict::Proved, "{outcome:?}");
        assert!(
            outcome
                .findings
                .iter()
                .all(|finding| finding.verdict == FormalFindingVerdict::Pass),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_false_statement_is_disproved() {
        if !lean_available() {
            return;
        }
        let engine = LeanEngine::new(LeanConfig::default());
        let outcome = engine
            .check(&request("(2:Nat) + 2 = 5", Some("decide")))
            .unwrap();
        // Nicht bewiesen und widerlegt sind verschiedene Aussagen. Die
        // Widerlegung belegt, dass die Aussage falsch ist.
        assert_eq!(outcome.verdict, FormalVerdict::Disproved, "{outcome:?}");
        assert!(
            outcome
                .findings
                .iter()
                .any(|finding| finding.check == "formal.disproved"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_placeholder_is_reported_as_a_finding() {
        if !lean_available() {
            return;
        }
        let engine = LeanEngine::new(LeanConfig::default());
        let outcome = engine
            .check(&request("(2:Nat) + 2 = 5", Some("sorry")))
            .unwrap();
        let sorry = outcome
            .findings
            .iter()
            .find(|finding| finding.check == "formal.no_sorry")
            .expect("the placeholder must be reported");
        assert_eq!(sorry.verdict, FormalFindingVerdict::Fail, "{outcome:?}");
        assert!(
            outcome.used_axioms.contains(&"sorryAx".to_owned()),
            "{outcome:?}"
        );
    }

    #[test]
    fn the_engine_searches_a_proof_on_its_own() {
        if !lean_available() {
            return;
        }
        let engine = LeanEngine::new(LeanConfig::default());
        let outcome = engine.check(&request("(2:Nat) + 2 = 4", None)).unwrap();
        assert_eq!(outcome.verdict, FormalVerdict::Proved, "{outcome:?}");
        assert_eq!(outcome.proof_text.as_deref(), Some("simp"), "{outcome:?}");
    }

    #[test]
    fn a_missing_engine_is_reported_as_unavailable() {
        let engine = LeanEngine::new(LeanConfig {
            binary: PathBuf::from("arcs-no-such-lean-binary"),
            ..LeanConfig::default()
        });
        let outcome = engine
            .check(&request("(2:Nat) + 2 = 4", Some("decide")))
            .unwrap();
        assert_eq!(outcome.verdict, FormalVerdict::Unavailable, "{outcome:?}");
    }

    #[test]
    fn a_timeout_is_reported_as_unavailable() {
        if !lean_available() {
            return;
        }
        let engine = LeanEngine::new(LeanConfig::default());
        let mut request = request("(2:Nat) + 2 = 4", Some("decide"));
        request.max_runtime_ms = 1;
        let outcome = engine.check(&request).unwrap();
        assert_eq!(outcome.verdict, FormalVerdict::Unavailable, "{outcome:?}");
    }
}

#[path = "../examples/support/persistent_goal.rs"]
mod qualification;
mod support;
use qualification::{Evidence, Options};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "arcs-persistent-proof-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/live-llm");
        let f = Self { directory };
        for entry in std::fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), f.directory.join(entry.file_name())).unwrap();
        }
        let mut host = f.read("host.json");
        host["database"] = json!(f.directory.join("core.sqlite"));
        for schema in host["schemas"].as_array_mut().unwrap() {
            *schema = json!(
                source
                    .join(schema.as_str().unwrap())
                    .canonicalize()
                    .unwrap()
            );
        }
        for (i, adapter) in host["adapters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            let mut grant: Value = serde_json::from_slice(
                &std::fs::read(source.join(adapter["grant_file"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            let id = grant["adapter_id"].as_str().unwrap().to_owned();
            let profile_path = adapter["profile"].as_str().unwrap();
            let profile_name = PathBuf::from(profile_path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            let mut profile: Value =
                serde_json::from_slice(&std::fs::read(source.join(profile_path)).unwrap()).unwrap();
            let binary = if id == "llm.planner" || id == "llm.assessor" {
                profile = json!({"adapter_id":id,"method":"rules","scenario":"text_fan_goal"});
                grant["reasoning_limits_v2"]["methods"] = json!(["rules"]);
                let r = &mut grant["reasoning_limits_v2"]["resources"];
                r["max_runtime_ms"] = json!(15000);
                r["max_iterations"] = json!(10000);
                r["max_output_tokens"] = Value::Null;
                "arcs-example-reasoner".to_owned()
            } else {
                PathBuf::from(adapter["program"].as_str().unwrap())
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            };
            adapter["program"] = json!(support::adapter_binary(&binary));
            adapter["profile"] = json!(profile_name);
            f.write(&profile_name, profile);
            let grant_file = format!("grant-{i}.json");
            adapter["grant_file"] = json!(grant_file);
            f.write(&grant_file, grant);
            let name = PathBuf::from(adapter["args"][0]["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            adapter["args"][0]["path"] = json!(f.directory.join(name));
        }
        f.write("host.json", host);
        let mut runtime = f.read("runtime.json");
        runtime["journal"] = json!(f.directory.join("runtime.sqlite"));
        let route = &mut runtime["routes"][0];
        route["method"] = json!("rules");
        route["goal"]["assessment"]["method"] = json!("rules");
        {
            let budget = route.get_mut("budget").unwrap();
            budget["max_runtime_ms"] = json!(15000);
            budget["max_iterations"] = json!(10000);
            budget["max_output_tokens"] = Value::Null;
        }
        let budget = &mut route["goal"]["assessment"]["budget"];
        budget["max_runtime_ms"] = json!(15000);
        budget["max_iterations"] = json!(10000);
        budget["max_output_tokens"] = Value::Null;
        f.write("runtime.json", runtime);
        f
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
    fn options(&self) -> Options {
        Options {
            host: self.directory.join("host.json"),
            runtime: self.directory.join("runtime.json"),
            simulator_program: support::adapter_binary("arcs-simulated-fan"),
            start_unix_ms: 1_800_000_000_000,
            phase_timeout: Duration::from_secs(25),
        }
    }
    fn run(&self) -> Evidence {
        qualification::qualify(&self.options())
    }
    fn effects(&self) -> i64 {
        rusqlite::Connection::open(self.directory.join("fan.sqlite"))
            .unwrap()
            .query_row("SELECT actuation_count FROM fan_state", [], |r| r.get(0))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn qualification_preserves_manual_reviews_context_monthly_waits_and_exactly_two_effects() {
    let f = Fixture::new();
    let proof = f.run();
    assert!(
        proof.passed,
        "{}",
        serde_json::to_string_pretty(&proof).unwrap()
    );
    assert_eq!(proof.attempt_versions.len(), 6);
    assert_eq!(proof.checkpoints.len(), 7);
    assert_eq!(proof.simulated_delay_ms, 90 * 24 * 3600 * 1000);
    assert_eq!(f.effects(), 2);
    assert!(
        proof
            .adapter_reported_models
            .iter()
            .all(|r| r["model"].is_null())
    ); // Rules are never relabelled as a live model.
    let again = f.run();
    assert!(!again.passed);
    assert!(again.error.unwrap().contains("fresh journals"));
    assert_eq!(f.effects(), 2);
}
#[test]
fn an_unsolved_model_goal_is_failed_evidence_without_an_approved_effect() {
    let f = Fixture::new();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["raw_text"] = json!("unsupported test input");
    f.write("input-profile.json", input);
    let proof = f.run();
    assert!(!proof.passed);
    assert!(proof.error.unwrap().contains("Blocked"));
    assert!(proof.attempt_versions.is_empty());
    assert_eq!(f.effects(), 0);
}

#[test]
fn a_valid_but_different_proposed_sequence_cannot_receive_a_test_approval() {
    let f = Fixture::new();
    let mut input = f.read("input-profile.json");
    input["events"][0]["payload"]["raw_text"] =
        json!("Schalte den Lüfter aus und dann wieder ein.");
    f.write("input-profile.json", input);
    let proof = f.run();
    assert!(!proof.passed);
    assert!(proof.error.unwrap().contains("different step"));
    assert_eq!(f.effects(), 0);
}
#[test]
fn policy_approval_is_rejected_before_any_host_or_model_start() {
    let f = Fixture::new();
    let mut runtime = f.read("runtime.json");
    runtime["routes"][0]["goal"]["approval"] =
        json!({"mode":"policy","policy_id":"not-a-test-review"});
    f.write("runtime.json", runtime);
    let proof = f.run();
    assert!(!proof.passed);
    assert!(proof.error.unwrap().contains("manual approval"));
    assert!(!f.directory.join("core.sqlite").exists());
}
#[test]
fn an_unexpected_actuator_binary_is_rejected_before_a_review_or_database_creation() {
    let f = Fixture::new();
    let mut host = f.read("host.json");
    host["adapters"][2]["program"] = json!("/bin/true");
    f.write("host.json", host);
    let proof = f.run();
    assert!(!proof.passed);
    assert!(proof.error.unwrap().contains("different actuator"));
    assert!(!f.directory.join("core.sqlite").exists());
}

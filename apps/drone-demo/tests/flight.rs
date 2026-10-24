use arcs::adapters::{AdapterId, CapabilityRef};
use arcs::core::{SchemaId, VersionId};
use arcs::store::{ArtifactRelations, relation_kinds};
use arcs_drone_demo::FlightConfig;
use arcs_host::Host;
use arcs_simulated_drone::{POSE_SCHEMA, Profile, Telemetry};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

fn adapter_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let profile = exe.parent().unwrap().parent().unwrap();
            let mut command = Command::new(env!("CARGO"));
            command
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args([
                    "build",
                    "--offline",
                    "--locked",
                    "-p",
                    "arcs-simulated-drone",
                    "--bin",
                    "arcs-simulated-drone",
                    "--target-dir",
                ])
                .arg(profile.parent().unwrap());
            if profile.file_name().unwrap() != "debug" {
                command.arg("--profile").arg(profile.file_name().unwrap());
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            profile.join(format!(
                "arcs-simulated-drone{}",
                std::env::consts::EXE_SUFFIX
            ))
        })
        .clone()
}
struct Fixture {
    directory: PathBuf,
    profile: Profile,
    config: FlightConfig,
    grant: Value,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-drone-flight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut fixture = Self {
            directory,
            profile: serde_json::from_str(include_str!(
                "../../../adapters/simulated-drone/config/profile.json"
            ))
            .unwrap(),
            config: serde_json::from_str(include_str!("../config/flight.json")).unwrap(),
            grant: serde_json::from_str(include_str!("../config/grant.json")).unwrap(),
        };
        fixture.config.cycle_id = "test-move-1".into();
        fixture.write("host.json",&json!({"config_version":1,"database":"db/core.sqlite","node_id":"flight-test",
            "schemas":[root.join("adapters/simulated-drone/schemas/pose.schema.json"),root.join("adapters/simulated-drone/schemas/query.schema.json"),root.join("adapters/simulated-drone/schemas/move-candidate.schema.json"),root.join("adapters/simulated-drone/schemas/result.schema.json"),root.join("apps/drone-demo/config/cycle.schema.json")],
            "adapters":[{"program":adapter_binary(),"profile":"profile.json","args":[{"path":"db/adapter.sqlite"}],"grant_file":"grant.json"}]}));
        fixture.save();
        fixture
    }
    fn write(&self, name: &str, value: &Value) {
        std::fs::write(
            self.directory.join(name),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
    fn save(&self) {
        self.write(
            "profile.json",
            &serde_json::to_value(&self.profile).unwrap(),
        );
        self.write("grant.json", &self.grant);
    }
    fn open(&self) -> Host {
        Host::open(self.directory.join("host.json")).unwrap()
    }
    fn state(&self) -> Telemetry {
        let value: String = Connection::open(self.directory.join("db/adapter.sqlite"))
            .unwrap()
            .query_row("SELECT state_json FROM drone_state", [], |row| row.get(0))
            .unwrap();
        serde_json::from_str(&value).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn version(report: &Value, key: &str) -> VersionId {
    VersionId(report["artifacts"][key].as_str().unwrap().into())
}

#[test]
fn real_adapter_moves_once_and_effect_is_backed_by_readback_and_observation() {
    let fixture = Fixture::new();
    let mut host = fixture.open();
    let first = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    assert_eq!(first["status"], "success");
    assert_eq!(first["before"]["pose"]["x_m"], 1.0);
    assert_eq!(first["after"]["pose"]["x_m"], 1.5);
    assert_eq!(first["after"]["revision"], 1);
    assert_eq!(first["host"]["world_revision"], 2);
    for (from, to, kind) in [
        ("candidate", "before", relation_kinds::supported_by()),
        ("candidate", "cycle", relation_kinds::supported_by()),
        ("approval", "candidate", relation_kinds::approves()),
        ("action", "approval", relation_kinds::based_on()),
        ("execution", "action", relation_kinds::result_of()),
        ("outcome", "execution", relation_kinds::evaluates()),
        ("outcome", "after", relation_kinds::supported_by()),
        ("outcome", "observation", relation_kinds::supported_by()),
    ] {
        assert!(
            ArtifactRelations::new(host.store())
                .outgoing(&version(&first, from))
                .unwrap()
                .iter()
                .any(|edge| edge.to == version(&first, to) && edge.kind == kind)
        );
    }
    assert!(
        host.report()
            .unwrap()
            .estimates
            .iter()
            .any(|estimate| estimate.schema_id.0 == POSE_SCHEMA
                && estimate.value["revision"] == 1
                && estimate.value["pose"]["x_m"] == 1.5)
    );
    host.shutdown().unwrap();
    drop(host);
    let mut host = fixture.open();
    let second = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    assert_eq!(second["artifacts"], first["artifacts"]);
    assert_eq!(
        second["host"]["stored_artifacts"],
        first["host"]["stored_artifacts"]
    );
    assert_eq!(second["host"]["world_revision"], 2);
    assert_eq!(fixture.state().revision, 1);
    host.shutdown().unwrap();
}

#[test]
fn rejected_or_out_of_policy_moves_never_materialize_an_action() {
    for approved in [false, true] {
        let mut fixture = Fixture::new();
        fixture.config.approve = approved;
        if approved {
            fixture.config.max_auto_step_m = 0.25;
        }
        let mut host = fixture.open();
        let report = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
        assert_eq!(
            report["status"],
            if approved {
                "verification_failed"
            } else {
                "rejected"
            }
        );
        assert!(report["artifacts"].get("action").is_none());
        assert!(report["artifacts"].get("execution").is_none());
        assert_eq!(fixture.state().revision, 0);
        assert!(
            host.execute_approved(
                &AdapterId("demo.drone".into()),
                &version(&report, "candidate")
            )
            .is_err()
        );
        host.shutdown().unwrap();
    }
}

#[test]
fn wall_or_obstacle_violation_is_rejected_by_driver_even_after_approval() {
    let mut fixture = Fixture::new();
    fixture.profile.initial_pose.x_m = 2.5;
    fixture.profile.initial_pose.y_m = 2.0;
    fixture.save();
    let mut host = fixture.open();
    let failure = arcs_drone_demo::run(&mut host, &fixture.config).unwrap_err();
    assert!(failure.contains("Rejected"));
    assert_eq!(fixture.state().revision, 0);
    assert_eq!(fixture.state().pose.x_m, 2.5);
    assert!(
        !host
            .store()
            .committed_after(0)
            .unwrap()
            .iter()
            .any(|event| event.artifact.artifact_type == "outcome")
    );
    // Der allgemeine stdio-Host schließt die Verbindung nach Adapterfehlern.
    drop(host);
}

#[test]
fn changed_policy_requires_a_new_cycle_and_new_queries_observe_the_next_revision() {
    let mut fixture = Fixture::new();
    fixture.config.approve = false;
    let mut host = fixture.open();
    let first = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    fixture.config.approve = true;
    assert!(
        arcs_drone_demo::run(&mut host, &fixture.config)
            .unwrap_err()
            .contains("IdentityConflict")
    );
    assert_eq!(fixture.state().revision, 0);
    fixture.config.cycle_id = "test-move-2".into();
    let moved = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    assert_eq!(moved["status"], "success");
    assert_ne!(moved["artifacts"]["cycle"], first["artifacts"]["cycle"]);
    fixture.config.cycle_id = "test-move-3".into();
    let next = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    assert_eq!(next["before"]["revision"], 1);
    assert_eq!(next["after"]["revision"], 2);
    assert_eq!(next["after"]["pose"]["x_m"], 2.0);
    host.shutdown().unwrap();
}

#[test]
fn act_capability_cannot_be_used_as_request_and_revoked_moves_cannot_replay() {
    let mut fixture = Fixture::new();
    let mut host = fixture.open();
    let first = arcs_drone_demo::run(&mut host, &fixture.config).unwrap();
    let query = host
        .store()
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|event| event.artifact)
        .find(|artifact| artifact.schema_id.0 == arcs_simulated_drone::QUERY_SCHEMA)
        .unwrap();
    assert!(
        host.request_version(
            &CapabilityRef::new("demo.drone", "drone.move"),
            &query.version_id,
            &SchemaId(POSE_SCHEMA.into())
        )
        .is_err()
    );
    host.shutdown().unwrap();
    drop(host);
    fixture.grant["enabled_capabilities"] = json!(["pose.observe", "pose.read"]);
    fixture.grant["granted_permissions"] = json!(["drone.read"]);
    fixture.save();
    let mut host = fixture.open();
    assert!(
        host.execute_approved(&AdapterId("demo.drone".into()), &version(&first, "action"))
            .is_err()
    );
    assert_eq!(fixture.state().revision, 1);
    host.shutdown().unwrap();
}

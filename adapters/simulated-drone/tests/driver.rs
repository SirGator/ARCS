use arcs_adapter_protocol::{
    ActInvocation, CapabilityRef, RequestInvocation, SchemaId, SubjectId, VersionId,
};
use arcs_adapter_sdk::Adapter;
use arcs_simulated_drone::{
    CANDIDATE_SCHEMA, POSE_SCHEMA, Profile, QUERY_SCHEMA, RESULT_SCHEMA, SimulatedDrone,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    directory: PathBuf,
    profile: Profile,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "arcs-drone-driver-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self {
            directory,
            profile: serde_json::from_str(include_str!("../config/profile.json")).unwrap(),
        }
    }
    fn open(&self) -> SimulatedDrone {
        SimulatedDrone::open(self.profile.clone(), &self.directory.join("state.sqlite")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn action(id: &str, revision: u64, dx: f64, dy: f64) -> ActInvocation {
    let capability = CapabilityRef::new("demo.drone", "drone.move");
    ActInvocation {
        invocation_id: id.into(),
        capability: capability.clone(),
        action_version_id: VersionId(format!("action-{id}")),
        action_schema_id: SchemaId("arcs.action.v1".into()),
        payload: json!({"target_version":"candidate-1","approval_version":"approval-1","capability":capability,"input_schema_id":CANDIDATE_SCHEMA,
            "payload":{"drone_id":"drone-1","expected_revision":revision,"movement":{"dx_m":dx,"dy_m":dy,"dyaw_deg":0.0}}}),
        result_schema_id: SchemaId(RESULT_SCHEMA.into()),
    }
}
fn query(id: &str) -> RequestInvocation {
    RequestInvocation {
        invocation_id: id.into(),
        capability: CapabilityRef::new("demo.drone", "pose.read"),
        request_version_id: VersionId(format!("query-{id}")),
        request_schema_id: SchemaId(QUERY_SCHEMA.into()),
        subject: SubjectId("drone-1".into()),
        request_payload: json!({"drone_id":"drone-1"}),
        response_schema_id: SchemaId(POSE_SCHEMA.into()),
        max_response_bytes: 4096,
    }
}

#[test]
fn state_outbox_ack_and_execution_replay_survive_restart() {
    let fixture = Fixture::new();
    let mut drone = fixture.open();
    let initial = drone.poll().unwrap().unwrap();
    assert_eq!(initial.observation.payload["revision"], 0);
    assert_eq!(
        drone.poll().unwrap().unwrap().message_id,
        initial.message_id
    );
    drone.acknowledge(&initial.message_id).unwrap();
    drone.acknowledge(&initial.message_id).unwrap();
    assert!(drone.poll().unwrap().is_none());
    let invocation = action("move-1", 0, 0.5, 0.0);
    let response = drone.execute(invocation.clone()).unwrap();
    drop(drone);
    let mut drone = fixture.open();
    assert_eq!(drone.execute(invocation).unwrap(), response);
    let changed = drone.poll().unwrap().unwrap();
    assert_eq!(changed.observation.payload["pose"]["x_m"], 1.5);
    assert_eq!(changed.observation.payload["revision"], 1);
    drone.acknowledge(&changed.message_id).unwrap();
    assert!(drone.poll().unwrap().is_none());
}

#[test]
fn stale_conflicting_and_invalid_commands_do_not_move() {
    let fixture = Fixture::new();
    let mut drone = fixture.open();
    let invocation = action("move-1", 0, 0.5, 0.0);
    drone.execute(invocation.clone()).unwrap();
    assert!(
        drone
            .execute(action("move-2", 0, 0.5, 0.0))
            .unwrap_err()
            .message
            .contains("stale")
    );
    let mut conflict = invocation;
    conflict.payload["payload"]["movement"]["dx_m"] = json!(0.25);
    assert!(
        drone
            .execute(conflict)
            .unwrap_err()
            .message
            .contains("identity")
    );
    assert!(drone.execute(action("move-too-far", 1, 2.0, 0.0)).is_err());
    let state = drone.request(query("fresh")).unwrap().payload;
    assert_eq!(state["revision"], 1);
    assert_eq!(state["pose"]["x_m"], 1.5);
}

#[test]
fn full_path_collision_rejects_a_valid_endpoint_beyond_an_obstacle() {
    let mut fixture = Fixture::new();
    fixture.profile.room.max_step_m = 3.0;
    fixture.profile.initial_pose.y_m = 2.0;
    let mut drone = fixture.open();
    assert!(
        drone
            .execute(action("cross-obstacle", 0, 3.0, 0.0))
            .is_err()
    );
    assert_eq!(
        drone.request(query("after")).unwrap().payload["revision"],
        0
    );
}

#[test]
fn request_ids_are_immutable_snapshots_and_profile_changes_are_rejected() {
    let mut fixture = Fixture::new();
    let mut drone = fixture.open();
    let before = drone.request(query("snapshot")).unwrap();
    drone.execute(action("move-1", 0, 0.5, 0.0)).unwrap();
    assert_eq!(drone.request(query("snapshot")).unwrap(), before);
    assert_eq!(
        drone.request(query("new-snapshot")).unwrap().payload["revision"],
        1
    );
    let mut changed = query("snapshot");
    changed.request_payload["drone_id"] = json!("other");
    assert!(drone.request(changed).is_err());
    drop(drone);
    fixture.profile.room.width_m = 7.0;
    assert!(
        SimulatedDrone::open(
            fixture.profile.clone(),
            &fixture.directory.join("state.sqlite")
        )
        .is_err()
    );
}

#[test]
fn process_killed_after_commit_replays_the_lost_response_without_second_motion() {
    use arcs_adapter_protocol::stdio::{
        AdapterMessage, Envelope, HostMessage, WIRE_VERSION, read_frame, write_frame,
    };
    use std::io::BufReader;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = Fixture::new();
    let profile = fixture.directory.join("profile.json");
    std::fs::write(&profile, serde_json::to_vec(&fixture.profile).unwrap()).unwrap();
    let database = fixture.directory.join("state.sqlite");
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_arcs-simulated-drone"))
            .arg(profile)
            .arg(&database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let mut output = BufReader::new(child.0.stdout.take().unwrap());
    write_frame(
        &mut input,
        &Envelope {
            protocol_version: WIRE_VERSION,
            request_id: "hello".into(),
            message: HostMessage::Hello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<Envelope<AdapterMessage>>(&mut output)
            .unwrap()
            .message,
        AdapterMessage::Manifest { .. }
    ));
    let invocation = action("lost-response", 0, 0.5, 0.0);
    write_frame(
        &mut input,
        &Envelope {
            protocol_version: WIRE_VERSION,
            request_id: "execute".into(),
            message: HostMessage::Execute {
                invocation: invocation.clone(),
            },
        },
    )
    .unwrap();
    // Die Antwort wird absichtlich nicht gelesen, nur der dauerhafte Commit.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let state: String = rusqlite::Connection::open(&database)
            .unwrap()
            .query_row("SELECT state_json FROM drone_state", [], |row| row.get(0))
            .unwrap();
        if serde_json::from_str::<serde_json::Value>(&state).unwrap()["revision"] == 1 {
            break;
        }
        assert!(Instant::now() < deadline, "move did not commit");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let mut drone = fixture.open();
    let replay = drone.execute(invocation).unwrap();
    assert_eq!(replay.result_payload["result"]["revision"], 1);
    assert_eq!(replay.result_payload["result"]["pose"]["x_m"], 1.5);
    let mut revisions = Vec::new();
    while let Some(observation) = drone.poll().unwrap() {
        revisions.push(
            observation.observation.payload["revision"]
                .as_u64()
                .unwrap(),
        );
        drone.acknowledge(&observation.message_id).unwrap();
    }
    assert_eq!(revisions, [0, 1]);
    assert_eq!(
        drone.request(query("fresh-after-crash")).unwrap().payload["revision"],
        1
    );
}

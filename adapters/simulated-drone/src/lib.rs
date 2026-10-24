//! Fachverträge und Treiber der Simulation. Keine Core-Abhängigkeit.
mod driver;
pub use arcs_room_sim::{Movement, Obstacle, Pose, Room};
pub use driver::SimulatedDrone;

use arcs_adapter_protocol::{
    AdapterId, AdapterManifest, CapabilityContract, CapabilityDescriptor, CapabilityId, SchemaId,
    VersionId,
};
use serde::{Deserialize, Serialize};

pub const POSE_SCHEMA: &str = "arcs.state.drone_pose.v1";
pub const QUERY_SCHEMA: &str = "arcs.request.drone_pose.v1";
pub const CANDIDATE_SCHEMA: &str = "arcs.drone_move_candidate.v1";
pub const RESULT_SCHEMA: &str = "arcs.execution_result.drone.v1";
pub const SCHEMAS: &[&str] = &[
    include_str!("../schemas/pose.schema.json"),
    include_str!("../schemas/query.schema.json"),
    include_str!("../schemas/move-candidate.schema.json"),
    include_str!("../schemas/result.schema.json"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub adapter_id: AdapterId,
    pub drone_id: String,
    pub room: Room,
    pub initial_pose: Pose,
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        if self.drone_id.is_empty()
            || self.drone_id.len() > 64
            || !self
                .drone_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err("invalid drone ID".into());
        }
        self.manifest()
            .validate()
            .map_err(|error| format!("{error:?}"))?;
        self.room.validate(self.initial_pose)
    }
    pub fn manifest(&self) -> AdapterManifest {
        let descriptor = |id: &str, contract, permission: &str| CapabilityDescriptor {
            id: CapabilityId(id.into()),
            contract,
            required_permissions: vec![permission.into()],
        };
        AdapterManifest {
            protocol_version: 1,
            adapter_id: self.adapter_id.clone(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: vec![
                descriptor(
                    "pose.observe",
                    CapabilityContract::Observe {
                        emits: vec![SchemaId(POSE_SCHEMA.into())],
                    },
                    "drone.read",
                ),
                descriptor(
                    "pose.read",
                    CapabilityContract::Request {
                        accepts: vec![SchemaId(QUERY_SCHEMA.into())],
                        emits: vec![SchemaId(POSE_SCHEMA.into())],
                    },
                    "drone.read",
                ),
                descriptor(
                    "drone.move",
                    CapabilityContract::Act {
                        accepts: vec![SchemaId(CANDIDATE_SCHEMA.into())],
                        emits: vec![SchemaId(RESULT_SCHEMA.into())],
                        idempotent: true,
                    },
                    "drone.move",
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    pub drone_id: String,
    pub pose: Pose,
    pub height_m: f64,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseQuery {
    pub drone_id: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveCommand {
    pub drone_id: String,
    pub expected_revision: u64,
    pub movement: Movement,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResult {
    pub action_version: VersionId,
    pub result: Telemetry,
}

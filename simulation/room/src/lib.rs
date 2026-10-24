//! Deterministische Bewegung in einer Ebene. Keine ARCS- oder Adapterabhängigkeit.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub x_m: f64,
    pub y_m: f64,
    pub yaw_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    /// Raumkoordinaten, unabhängig von der Blickrichtung.
    pub dx_m: f64,
    pub dy_m: f64,
    pub dyaw_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obstacle {
    pub min_x_m: f64,
    pub min_y_m: f64,
    pub max_x_m: f64,
    pub max_y_m: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub width_m: f64,
    pub depth_m: f64,
    pub flight_height_m: f64,
    /// Konservativer achsenparalleler Freiraum um die Drohne.
    pub clearance_m: f64,
    pub max_step_m: f64,
    pub max_turn_deg: f64,
    pub obstacles: Vec<Obstacle>,
}

impl Room {
    pub fn validate(&self, initial: Pose) -> Result<(), String> {
        let positive = [
            self.width_m,
            self.depth_m,
            self.flight_height_m,
            self.clearance_m,
            self.max_step_m,
            self.max_turn_deg,
        ];
        if positive
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || self.width_m > 10_000.0
            || self.depth_m > 10_000.0
            || self.flight_height_m > 1000.0
            || self.max_turn_deg > 180.0
            || self.clearance_m * 2.0 >= self.width_m
            || self.clearance_m * 2.0 >= self.depth_m
            || self.obstacles.len() > 256
        {
            return Err("invalid room bounds or motion limits".into());
        }
        for obstacle in &self.obstacles {
            let values = [
                obstacle.min_x_m,
                obstacle.min_y_m,
                obstacle.max_x_m,
                obstacle.max_y_m,
            ];
            if values.iter().any(|value| !value.is_finite())
                || obstacle.min_x_m < 0.0
                || obstacle.min_y_m < 0.0
                || obstacle.max_x_m > self.width_m
                || obstacle.max_y_m > self.depth_m
                || obstacle.min_x_m >= obstacle.max_x_m
                || obstacle.min_y_m >= obstacle.max_y_m
            {
                return Err("invalid obstacle bounds".into());
            }
        }
        self.validate_pose(initial)
    }

    pub fn validate_pose(&self, pose: Pose) -> Result<(), String> {
        if !pose.x_m.is_finite()
            || !pose.y_m.is_finite()
            || !pose.yaw_deg.is_finite()
            || !(0.0..360.0).contains(&pose.yaw_deg)
            || pose.x_m <= self.clearance_m
            || pose.x_m >= self.width_m - self.clearance_m
            || pose.y_m <= self.clearance_m
            || pose.y_m >= self.depth_m - self.clearance_m
            || self
                .obstacles
                .iter()
                .any(|obstacle| self.intersects(pose, pose, obstacle))
        {
            return Err("pose outside free space".into());
        }
        Ok(())
    }

    /// Prüft die gesamte Strecke, nicht nur deren Endpunkt. Eine abgewiesene
    /// Bewegung verändert den Eingangszustand nicht; Zeit und Physik fehlen.
    pub fn moved(&self, pose: Pose, movement: Movement) -> Result<Pose, String> {
        self.validate_pose(pose)?;
        if !movement.dx_m.is_finite()
            || !movement.dy_m.is_finite()
            || !movement.dyaw_deg.is_finite()
            || movement.dx_m.hypot(movement.dy_m) > self.max_step_m
            || movement.dyaw_deg.abs() > self.max_turn_deg
            || (movement.dx_m == 0.0 && movement.dy_m == 0.0 && movement.dyaw_deg == 0.0)
        {
            return Err("invalid or excessive movement".into());
        }
        let result = Pose {
            x_m: pose.x_m + movement.dx_m,
            y_m: pose.y_m + movement.dy_m,
            yaw_deg: (pose.yaw_deg + movement.dyaw_deg).rem_euclid(360.0),
        };
        self.validate_pose(result)?;
        if result == pose {
            return Err("movement has no representable effect".into());
        }
        if self
            .obstacles
            .iter()
            .any(|obstacle| self.intersects(pose, result, obstacle))
        {
            return Err("movement crosses obstacle clearance".into());
        }
        Ok(result)
    }

    fn intersects(&self, from: Pose, to: Pose, obstacle: &Obstacle) -> bool {
        let mut enter: f64 = 0.0;
        let mut leave: f64 = 1.0;
        for (start, delta, minimum, maximum) in [
            (
                from.x_m,
                to.x_m - from.x_m,
                obstacle.min_x_m - self.clearance_m,
                obstacle.max_x_m + self.clearance_m,
            ),
            (
                from.y_m,
                to.y_m - from.y_m,
                obstacle.min_y_m - self.clearance_m,
                obstacle.max_y_m + self.clearance_m,
            ),
        ] {
            if delta == 0.0 {
                if start < minimum || start > maximum {
                    return false;
                }
            } else {
                let a = (minimum - start) / delta;
                let b = (maximum - start) / delta;
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
                if enter > leave {
                    return false;
                }
            }
        }
        true
    }
}

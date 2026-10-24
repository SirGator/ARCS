use arcs_room_sim::{Movement, Obstacle, Pose, Room};

fn room() -> Room {
    Room {
        width_m: 6.0,
        depth_m: 4.0,
        flight_height_m: 1.0,
        clearance_m: 0.2,
        max_step_m: 3.0,
        max_turn_deg: 90.0,
        obstacles: vec![Obstacle {
            min_x_m: 2.0,
            min_y_m: 1.5,
            max_x_m: 2.2,
            max_y_m: 2.5,
        }],
    }
}
fn pose() -> Pose {
    Pose {
        x_m: 1.0,
        y_m: 1.0,
        yaw_deg: 350.0,
    }
}

#[test]
fn bounded_movement_and_rotation_are_deterministic() {
    let room = room();
    room.validate(pose()).unwrap();
    let movement = Movement {
        dx_m: 0.5,
        dy_m: 0.0,
        dyaw_deg: 20.0,
    };
    let result = room.moved(pose(), movement).unwrap();
    assert_eq!(
        result,
        Pose {
            x_m: 1.5,
            y_m: 1.0,
            yaw_deg: 10.0
        }
    );
    assert_eq!(room.moved(pose(), movement).unwrap(), result);
}

#[test]
fn walls_clearance_and_obstacle_crossings_are_rejected() {
    let room = room();
    for (start, movement) in [
        (
            pose(),
            Movement {
                dx_m: -0.9,
                dy_m: 0.0,
                dyaw_deg: 0.0,
            },
        ),
        (
            Pose { y_m: 2.0, ..pose() },
            Movement {
                dx_m: 2.0,
                dy_m: 0.0,
                dyaw_deg: 0.0,
            },
        ),
        (
            pose(),
            Movement {
                dx_m: 1.0,
                dy_m: 0.3,
                dyaw_deg: 0.0,
            },
        ),
    ] {
        assert!(room.moved(start, movement).is_err());
    }
    assert!(
        room.moved(
            pose(),
            Movement {
                dx_m: 2.0,
                dy_m: 0.0,
                dyaw_deg: 0.0
            }
        )
        .is_ok()
    );
}

#[test]
fn malformed_scenarios_and_excessive_commands_are_rejected() {
    let mut room = room();
    for movement in [
        Movement {
            dx_m: f64::NAN,
            dy_m: 0.0,
            dyaw_deg: 0.0,
        },
        Movement {
            dx_m: f64::MIN_POSITIVE,
            dy_m: 0.0,
            dyaw_deg: 0.0,
        },
        Movement {
            dx_m: 4.0,
            dy_m: 0.0,
            dyaw_deg: 0.0,
        },
        Movement {
            dx_m: 0.0,
            dy_m: 0.0,
            dyaw_deg: 91.0,
        },
        Movement {
            dx_m: 0.0,
            dy_m: 0.0,
            dyaw_deg: 0.0,
        },
    ] {
        assert!(room.moved(pose(), movement).is_err());
    }
    room.obstacles[0].max_x_m = 7.0;
    assert!(room.validate(pose()).is_err());
    room.obstacles.clear();
    room.clearance_m = 4.0;
    assert!(room.validate(pose()).is_err());
}

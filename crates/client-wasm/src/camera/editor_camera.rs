use glam::Vec3;

use crate::input::InputState;

use super::controller::CameraController;
use super::projection::Camera;

/// Flexible free-roaming world editor camera supporting arrow-key panning, middle-drag orbit, and zoom.
#[derive(Debug, Clone)]
pub struct EditorCameraController {
    pub target: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
}

impl Default for EditorCameraController {
    fn default() -> Self {
        Self {
            // Center of tile (0, 0) in "main" where brand-new players spawn
            target: Vec3::new(32.0, 0.0, 32.0),
            yaw_deg: 45.0,
            pitch_deg: 45.0,
            distance: 52.0,
        }
    }
}

impl EditorCameraController {
    pub fn focus_on(&mut self, x: f32, z: f32) {
        self.target = Vec3::new(x, 0.0, z);
    }
}

impl CameraController for EditorCameraController {
    fn update(&mut self, camera: &mut Camera, input: &InputState, dt: f32) {
        if input.middle_mouse_down {
            self.yaw_deg += input.mouse_delta.0 * 0.35;
            self.pitch_deg = (self.pitch_deg + input.mouse_delta.1 * 0.30).clamp(10.0, 85.0);
        }

        if input.scroll_delta.abs() > 0.0 {
            let factor = (1.0 + input.scroll_delta * 0.12).clamp(0.7, 1.4);
            self.distance = (self.distance * factor).clamp(6.0, 260.0);
        }

        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();

        let forward = Vec3::new(-yaw.cos(), 0.0, -yaw.sin()).normalize_or_zero();
        let right = Vec3::new(yaw.sin(), 0.0, -yaw.cos()).normalize_or_zero();

        let mut move_dir = Vec3::ZERO;
        if input.is_key_down("ArrowUp") {
            move_dir += forward;
        }
        if input.is_key_down("ArrowDown") {
            move_dir -= forward;
        }
        if input.is_key_down("ArrowRight") {
            move_dir += right;
        }
        if input.is_key_down("ArrowLeft") {
            move_dir -= right;
        }

        if move_dir.length_squared() > 0.0 {
            let boost = if input.is_key_down("ShiftLeft") || input.is_key_down("ShiftRight") {
                2.2
            } else {
                1.0
            };
            let pan_speed = (self.distance * 0.85).clamp(18.0, 140.0) * boost;
            self.target += move_dir.normalize() * pan_speed * dt;
        }

        let offset = Vec3::new(
            self.distance * pitch.cos() * yaw.cos(),
            self.distance * pitch.sin(),
            self.distance * pitch.cos() * yaw.sin(),
        );

        camera.target = self.target;
        camera.eye = self.target + offset;
    }
}

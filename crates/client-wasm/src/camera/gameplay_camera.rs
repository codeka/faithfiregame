use glam::Vec3;

use crate::input::InputState;

use super::controller::CameraController;
use super::projection::Camera;

/// Loose third-person gameplay camera tethered around the player's world position.
#[derive(Debug, Clone)]
pub struct GameplayCameraController {
    pub player_pos: Vec3,
    pub current_target: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl Default for GameplayCameraController {
    fn default() -> Self {
        Self {
            player_pos: Vec3::new(32.0, 0.0, 32.0),
            current_target: Vec3::new(32.0, 0.0, 32.0),
            yaw_deg: 45.0,
            pitch_deg: 42.0,
            distance: 22.0,
            min_distance: 8.0,
            max_distance: 58.0,
        }
    }
}

impl GameplayCameraController {
    pub fn snap_to(&mut self, pos: Vec3, yaw_deg: f32, pitch_deg: f32, distance: f32) {
        self.player_pos = pos;
        self.current_target = pos + Vec3::Y * 1.2;
        self.yaw_deg = yaw_deg;
        self.pitch_deg = pitch_deg.clamp(18.0, 72.0);
        self.distance = distance.clamp(self.min_distance, self.max_distance);
    }
}

impl CameraController for GameplayCameraController {
    fn update(&mut self, camera: &mut Camera, input: &InputState, dt: f32) {
        if input.middle_mouse_down {
            self.yaw_deg += input.mouse_delta.0 * 0.35;
            self.pitch_deg = (self.pitch_deg + input.mouse_delta.1 * 0.30).clamp(18.0, 72.0);
        }
        if input.scroll_delta.abs() > 0.0 {
            let factor = (1.0 + input.scroll_delta * 0.1).clamp(0.7, 1.4);
            self.distance = (self.distance * factor).clamp(self.min_distance, self.max_distance);
        }

        let lerp_t = (8.0 * dt).min(1.0);
        self.current_target = self.current_target.lerp(self.player_pos + Vec3::Y * 1.2, lerp_t);

        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();
        let offset = Vec3::new(
            self.distance * pitch.cos() * yaw.cos(),
            self.distance * pitch.sin(),
            self.distance * pitch.cos() * yaw.sin(),
        );

        camera.target = self.current_target;
        camera.eye = self.current_target + offset;
    }
}

use std::collections::HashSet;

use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Default, Clone)]
pub struct InputState {
    pub keys: HashSet<String>,
    pub middle_mouse_down: bool,
    pub left_mouse_down: bool,
    pub left_just_pressed: bool,
    pub left_just_released: bool,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_delta: (f32, f32),
    pub scroll_delta: f32,
    pub canvas_width: f32,
    pub canvas_height: f32,
}

impl InputState {
    pub fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
        self.left_just_pressed = false;
        self.left_just_released = false;
    }

    pub fn is_key_down(&self, code: &str) -> bool {
        self.keys.contains(code)
    }
}

#[derive(Debug, Clone)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub aspect: f32,
    pub fovy_deg: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Vec3::new(58.0, 36.0, 58.0),
            target: Vec3::new(32.0, 0.0, 32.0),
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy_deg: 45.0,
            znear: 0.25,
            zfar: 600.0,
        }
    }
}

impl Camera {
    pub fn view_proj_matrix(&self) -> Mat4 {
        let proj = Mat4::perspective_rh_gl(
            self.fovy_deg.to_radians(),
            self.aspect.max(0.1),
            self.znear,
            self.zfar,
        );
        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        proj * view
    }

    /// Computes a normalized world-space ray `(origin, direction)` from viewport pixel coordinates.
    pub fn screen_ray(&self, mouse_x: f32, mouse_y: f32, width: f32, height: f32) -> Option<(Vec3, Vec3)> {
        if width <= 1.0 || height <= 1.0 {
            return None;
        }
        let ndc_x = (mouse_x / width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (mouse_y / height) * 2.0;
        let inv_vp = self.view_proj_matrix().inverse();

        let near_h = inv_vp * Vec4::new(ndc_x, ndc_y, -1.0, 1.0);
        let far_h = inv_vp * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
        if near_h.w.abs() < 1e-6 || far_h.w.abs() < 1e-6 {
            return None;
        }

        let near = near_h.truncate() / near_h.w;
        let far = far_h.truncate() / far_h.w;
        let dir = (far - near).normalize_or_zero();
        if dir.length_squared() < 1e-6 {
            return None;
        }
        Some((near, dir))
    }
}

pub trait CameraController {
    fn update(&mut self, camera: &mut Camera, input: &InputState, dt: f32);
}

/// Loose third-person gameplay camera tethered around the player's world position.
#[derive(Debug, Clone)]
pub struct GameplayCameraController {
    pub player_pos: Vec3,
    pub current_target: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
}

impl Default for GameplayCameraController {
    fn default() -> Self {
        Self {
            player_pos: Vec3::new(32.0, 0.0, 32.0),
            current_target: Vec3::new(32.0, 0.0, 32.0),
            yaw_deg: 45.0,
            pitch_deg: 42.0,
            distance: 22.0,
        }
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
            self.distance = (self.distance * factor).clamp(8.0, 58.0);
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

/// Flexible free-roaming world editor camera supporting arrow-key panning, middle-drag orbit, and zoom.
#[cfg(feature = "editor")]
#[derive(Debug, Clone)]
pub struct EditorCameraController {
    pub target: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
}

#[cfg(feature = "editor")]
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

#[cfg(feature = "editor")]
impl EditorCameraController {
    pub fn focus_on(&mut self, x: f32, z: f32) {
        self.target = Vec3::new(x, 0.0, z);
    }
}

#[cfg(feature = "editor")]
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

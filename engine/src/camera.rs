use glam::Mat4;
use crate::renderer::InputState;

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: glam::Mat4 = glam::Mat4::from_cols(
    glam::Vec4::new(1.0, 0.0, 0.0, 0.0),
    glam::Vec4::new(0.0, 1.0, 0.0, 0.0),
    glam::Vec4::new(0.0, 0.0, 0.5, 0.0),
    glam::Vec4::new(0.0, 0.0, 0.5, 1.0),
);

#[derive(Debug, Clone)]
pub struct Camera {
    pub eye: glam::Vec3,
    pub target: glam::Vec3,
    pub up: glam::Vec3,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera {
    pub fn new(eye: glam::Vec3, target: glam::Vec3, aspect: f32) -> Self {
        Self {
            eye,
            target,
            up: glam::Vec3::Z, // Z is Up
            aspect,
            fovy: 45.0,
            znear: 0.1,
            zfar: 2000.0, // Increase far plane to view larger terrains
        }
    }

    pub fn build_view_projection_matrix(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fovy.to_radians(), self.aspect, self.znear, self.zfar);
        OPENGL_TO_WGPU_MATRIX * proj * view
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().to_cols_array_2d();
    }
}

pub struct CameraController {
    pub yaw: f32,       // in degrees
    pub pitch: f32,     // in degrees
    pub distance: f32,
    pub pan_speed: f32,
    pub rotate_speed: f32,
    pub zoom_speed: f32,
}

impl CameraController {
    pub fn new(yaw: f32, pitch: f32, distance: f32) -> Self {
        Self {
            yaw,
            pitch,
            distance,
            pan_speed: 150.0,
            rotate_speed: 0.15,
            zoom_speed: 15.0,
        }
    }

    pub fn update_camera(
        &mut self,
        camera: &mut Camera,
        target: &mut glam::Vec3,
        input: &mut InputState,
        dt: f32,
    ) {
        // Rotate (if right mouse is pressed)
        if input.right_mouse_pressed {
            self.yaw -= input.mouse_delta.0 * self.rotate_speed;
            self.pitch = (self.pitch + input.mouse_delta.1 * self.rotate_speed).clamp(5.0, 85.0);
        }

        // Zoom
        self.distance = (self.distance - input.scroll_delta * self.zoom_speed).clamp(10.0, 1500.0);

        // Reset frame-based inputs
        input.mouse_delta = (0.0, 0.0);
        input.scroll_delta = 0.0;

        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();

        // Calculate panning directions relative to current yaw
        let forward_dir = glam::Vec3::new(yaw_rad.cos(), yaw_rad.sin(), 0.0);
        let right_dir = glam::Vec3::new(-yaw_rad.sin(), yaw_rad.cos(), 0.0);

        let mut move_dir = glam::Vec3::ZERO;
        if input.w_pressed {
            move_dir += forward_dir;
        }
        if input.s_pressed {
            move_dir -= forward_dir;
        }
        if input.a_pressed {
            move_dir -= right_dir;
        }
        if input.d_pressed {
            move_dir += right_dir;
        }

        if move_dir.length_squared() > 0.0 {
            *target += move_dir.normalize() * self.pan_speed * dt;
        }

        // Update camera position
        camera.target = *target;
        camera.eye = *target
            + glam::Vec3::new(
                self.distance * pitch_rad.cos() * yaw_rad.cos(),
                self.distance * pitch_rad.cos() * yaw_rad.sin(),
                self.distance * pitch_rad.sin(),
            );
    }
}

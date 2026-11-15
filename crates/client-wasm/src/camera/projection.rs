use glam::{Mat4, Vec3, Vec4};

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

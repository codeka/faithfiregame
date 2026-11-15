use glam::{Mat4, Vec3};
use glow::HasContext;

use crate::camera::Camera;

use super::shaders::{SHADOW_FRAG, SHADOW_VERT, compile_program};

pub const SHADOW_MAP_SIZE: i32 = 2048;

/// Unit vector pointing from world surfaces toward the directional sun.
pub fn sun_direction() -> Vec3 {
    Vec3::new(-0.72, 0.48, 0.50).normalize()
}

/// GPU resources for the directional depth shadow map pass.
pub(super) struct ShadowMapGpu {
    pub(super) fbo: glow::Framebuffer,
    pub(super) depth_tex: glow::Texture,
    pub(super) prog: glow::Program,
    pub(super) u_light_vp_loc: Option<glow::UniformLocation>,
    pub(super) u_model_loc: Option<glow::UniformLocation>,
    pub(super) size: i32,
}

impl ShadowMapGpu {
    /// Creates the 2048x2048 `DEPTH_COMPONENT24` shadow framebuffer and depth shader program.
    pub(super) fn new(gl: &glow::Context) -> Result<Self, String> {
        let prog = compile_program(gl, SHADOW_VERT, SHADOW_FRAG)?;
        let size = SHADOW_MAP_SIZE;

        unsafe {
            let u_light_vp_loc = gl.get_uniform_location(prog, "u_light_view_proj");
            let u_model_loc = gl.get_uniform_location(prog, "u_model");

            let depth_tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(depth_tex));
            gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::DEPTH_COMPONENT24, size, size);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_COMPARE_MODE,
                glow::COMPARE_REF_TO_TEXTURE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_COMPARE_FUNC,
                glow::LEQUAL as i32,
            );

            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::TEXTURE_2D,
                Some(depth_tex),
                0,
            );
            gl.draw_buffers(&[glow::NONE]);
            gl.read_buffer(glow::NONE);

            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("shadow framebuffer incomplete: 0x{status:x}"));
            }

            Ok(Self {
                fbo,
                depth_tex,
                prog,
                u_light_vp_loc,
                u_model_loc,
                size,
            })
        }
    }
}

/// Computes an orthographic WebGL clip-space light matrix centered on `camera.target`
/// and snapped to shadow-map texels so panning does not shimmer shadow edges.
pub(super) fn compute_light_space_matrix(camera: &Camera, shadow_map_size: i32) -> Mat4 {
    let cam_dist = (camera.eye - camera.target).length();
    let half_extent = ((cam_dist * 1.25).clamp(28.0, 140.0) / 4.0).ceil() * 4.0;

    let sun_dir = sun_direction();
    let light_forward = -sun_dir;
    let world_up = if sun_dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let light_right = light_forward.cross(world_up).normalize();
    let light_up = light_right.cross(light_forward).normalize();

    let texel_size = (half_extent * 2.0) / (shadow_map_size.max(1) as f32);
    let rx = (camera.target.dot(light_right) / texel_size).floor() * texel_size;
    let ry = (camera.target.dot(light_up) / texel_size).floor() * texel_size;
    let rz = camera.target.dot(light_forward).round();
    let snapped_center = light_right * rx + light_up * ry + light_forward * rz;

    let depth_half = 140.0;
    let light_eye = snapped_center - light_forward * (depth_half + 1.0);
    let view = Mat4::look_at_rh(light_eye, snapped_center, light_up);
    let proj = Mat4::orthographic_rh_gl(
        -half_extent,
        half_extent,
        -half_extent,
        half_extent,
        1.0,
        depth_half * 2.0 + 1.0,
    );
    proj * view
}

#[cfg(test)]
mod tests {
    use glam::Vec4;

    use super::*;

    fn project_ndc(vp: &Mat4, p: Vec3) -> Vec3 {
        let clip = *vp * Vec4::new(p.x, p.y, p.z, 1.0);
        clip.truncate() / clip.w
    }

    #[test]
    fn camera_target_projects_near_light_center() {
        let cam = Camera::default();
        let lvp = compute_light_space_matrix(&cam, SHADOW_MAP_SIZE);
        let ndc = project_ndc(&lvp, cam.target);
        assert!(ndc.x.abs() < 0.02, "expected ndc.x near 0, got {}", ndc.x);
        assert!(ndc.y.abs() < 0.02, "expected ndc.y near 0, got {}", ndc.y);
        assert!(ndc.z.abs() < 0.02, "expected ndc.z near 0, got {}", ndc.z);
    }

    #[test]
    fn elevated_caster_has_smaller_depth_than_ground_receiver() {
        let cam = Camera::default();
        let lvp = compute_light_space_matrix(&cam, SHADOW_MAP_SIZE);
        let receiver = cam.target;
        let caster = receiver + sun_direction() * 8.0;
        let ndc_receiver = project_ndc(&lvp, receiver);
        let ndc_caster = project_ndc(&lvp, caster);
        assert!(
            ndc_caster.z < ndc_receiver.z,
            "caster depth {} should be < receiver depth {}",
            ndc_caster.z,
            ndc_receiver.z
        );
    }

    #[test]
    fn subtexel_pan_does_not_drift_projected_xy() {
        let mut cam_a = Camera::default();
        let cam_dist = (cam_a.eye - cam_a.target).length();
        let half_extent = ((cam_dist * 1.25).clamp(28.0, 140.0) / 4.0).ceil() * 4.0;
        let texel = (half_extent * 2.0) / SHADOW_MAP_SIZE as f32;

        let sun_dir = sun_direction();
        let light_forward = -sun_dir;
        let light_right = light_forward.cross(Vec3::Y).normalize();
        let light_up = light_right.cross(light_forward).normalize();

        // Nudge camera away from any floor() boundary, then test a sub-texel delta inside that texel.
        let base_shift = (light_right + light_up) * (texel * 0.35);
        cam_a.target += base_shift;
        cam_a.eye += base_shift;

        let mut cam_b = cam_a.clone();
        let sub_shift = (light_right + light_up) * (texel * 0.15);
        cam_b.target += sub_shift;
        cam_b.eye += sub_shift;

        let lvp_a = compute_light_space_matrix(&cam_a, SHADOW_MAP_SIZE);
        let lvp_b = compute_light_space_matrix(&cam_b, SHADOW_MAP_SIZE);

        let world_pt = Vec3::new(35.0, 2.0, 29.0);
        let ndc_a = project_ndc(&lvp_a, world_pt);
        let ndc_b = project_ndc(&lvp_b, world_pt);
        assert!((ndc_a.x - ndc_b.x).abs() < 1e-6);
        assert!((ndc_a.y - ndc_b.y).abs() < 1e-6);
    }
}

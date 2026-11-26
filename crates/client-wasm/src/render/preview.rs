use glam::{Quat, Vec3};
use glow::HasContext;

use crate::camera::Camera;

pub struct PreviewTargetGpu {
    pub(super) fbo: glow::Framebuffer,
    color_tex: glow::Texture,
    depth_rb: glow::Renderbuffer,
    pub(super) width: i32,
    pub(super) height: i32,
}

impl PreviewTargetGpu {
    pub fn ensure(
        slot: &mut Option<Self>,
        gl: &glow::Context,
        width: i32,
        height: i32,
    ) -> Result<(), String> {
        let w = width.clamp(16, 512);
        let h = height.clamp(16, 512);
        if let Some(existing) = slot.as_ref() {
            if existing.width == w && existing.height == h {
                return Ok(());
            }
        }
        if let Some(old) = slot.take() {
            old.destroy(gl);
        }
        unsafe {
            let color_tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(color_tex));
            gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::RGBA8, w, h);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);

            let depth_rb = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth_rb));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, w, h);
            gl.bind_renderbuffer(glow::RENDERBUFFER, None);

            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color_tex),
                0,
            );
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth_rb),
            );
            gl.draw_buffers(&[glow::COLOR_ATTACHMENT0]);
            gl.read_buffer(glow::COLOR_ATTACHMENT0);

            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(fbo);
                gl.delete_texture(color_tex);
                gl.delete_renderbuffer(depth_rb);
                return Err(format!("preview framebuffer incomplete: 0x{status:x}"));
            }

            *slot = Some(Self {
                fbo,
                color_tex,
                depth_rb,
                width: w,
                height: h,
            });
        }
        Ok(())
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.color_tex);
            gl.delete_renderbuffer(self.depth_rb);
        }
    }
}

/// Computes a tight-framing square camera and front-key sun direction for a `.glb` bounding box.
pub fn compute_item_thumbnail_camera(aabb_min: Vec3, aabb_max: Vec3) -> (Camera, Vec3) {
    let center = (aabb_min + aabb_max) * 0.5;
    let ext = (aabb_max - aabb_min).max(Vec3::splat(0.02));

    let is_long_z = ext.z > ext.x * 1.8 && ext.z > ext.y * 1.4;
    let is_back_facing = aabb_max.z <= 0.02 && aabb_min.z < -0.05;

    let view_dir = if is_long_z {
        Vec3::new(0.92, 0.22, 0.32).normalize()
    } else if is_back_facing {
        Vec3::new(-0.36, 0.18, -0.91).normalize()
    } else {
        Vec3::new(0.32, 0.16, 0.93).normalize()
    };

    let raw_up = if is_long_z {
        Quat::from_axis_angle(view_dir, 24f32.to_radians()) * Vec3::Y
    } else {
        Vec3::Y
    };

    let forward = -view_dir;
    let right = forward.cross(raw_up).normalize();
    let up = right.cross(forward).normalize();

    let mut half_w = 0.0f32;
    let mut half_h = 0.0f32;
    let mut half_d = 0.0f32;
    for sx in [aabb_min.x, aabb_max.x] {
        for sy in [aabb_min.y, aabb_max.y] {
            for sz in [aabb_min.z, aabb_max.z] {
                let d = Vec3::new(sx, sy, sz) - center;
                half_w = half_w.max(d.dot(right).abs());
                half_h = half_h.max(d.dot(up).abs());
                half_d = half_d.max(d.dot(forward).abs());
            }
        }
    }

    let fovy_deg = 32.0f32;
    let half_tan = (0.5 * fovy_deg.to_radians()).tan();
    let dist = (half_h.max(half_w) / half_tan) * 1.22 + half_d;

    let cam = Camera {
        eye: center + view_dir * dist,
        target: center,
        up,
        aspect: 1.0,
        fovy_deg,
        znear: (dist - half_d * 1.5).max(0.02),
        zfar: dist + half_d * 2.5 + 1.0,
    };
    let sun_dir = (view_dir * 0.72 + right * 0.45 + up * 0.52).normalize();
    (cam, sun_dir)
}

#[cfg(test)]
mod tests {
    use glam::Vec4;

    use super::*;

    #[test]
    fn thumbnail_camera_frames_all_item_categories_inside_viewport() {
        let sample_aabbs = [
            ("torso", Vec3::new(-0.292, 0.9, -0.125), Vec3::new(0.292, 1.52, 0.135)),
            ("legs", Vec3::new(-0.177, 0.0, -0.11), Vec3::new(0.177, 0.96, 0.16)),
            ("guard_rifle", Vec3::new(-0.032, -0.085, -0.29), Vec3::new(0.032, 0.146, 0.58)),
            ("scrap_blade", Vec3::new(-0.02, -0.055, -0.08), Vec3::new(0.02, 0.055, 0.74)),
            ("salvage_pack", Vec3::new(-0.15, -0.22, -0.17), Vec3::new(0.215, 0.18, 0.0)),
        ];

        for (name, mn, mx) in sample_aabbs {
            let (cam, sun) = compute_item_thumbnail_camera(mn, mx);
            assert!((sun.length() - 1.0).abs() < 1e-4, "{name} sun normalized");
            let vp = cam.view_proj_matrix();
            let mut min_x = f32::MAX;
            let mut max_x = f32::MIN;
            let mut min_y = f32::MAX;
            let mut max_y = f32::MIN;
            for sx in [mn.x, mx.x] {
                for sy in [mn.y, mx.y] {
                    for sz in [mn.z, mx.z] {
                        let clip = vp * Vec4::new(sx, sy, sz, 1.0);
                        let ndc = clip.truncate() / clip.w;
                        assert!(
                            ndc.x.abs() <= 0.95 && ndc.y.abs() <= 0.95 && ndc.z.abs() <= 1.0,
                            "{name} corner clipped: {ndc:?}"
                        );
                        min_x = min_x.min(ndc.x);
                        max_x = max_x.max(ndc.x);
                        min_y = min_y.min(ndc.y);
                        max_y = max_y.max(ndc.y);
                    }
                }
            }
            assert!(
                (max_x - min_x).max(max_y - min_y) > 0.55,
                "{name} coverage too small: x=[{min_x},{max_x}] y=[{min_y},{max_y}]"
            );
        }
    }
}

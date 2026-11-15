use std::f32::consts::{PI, TAU};

use glam::{EulerRot, Quat, Vec2, Vec3};

use crate::camera::Camera;

pub const ROTATE_RING_RADIUS: f32 = 0.88;
const ROTATE_PICK_SEGMENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
}

impl GizmoAxis {
    pub fn unit_dir(self) -> Vec3 {
        match self {
            GizmoAxis::X => Vec3::X,
            GizmoAxis::Y => Vec3::Y,
            GizmoAxis::Z => Vec3::Z,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            GizmoAxis::X => 0,
            GizmoAxis::Y => 1,
            GizmoAxis::Z => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GizmoDragState {
    pub axis: GizmoAxis,
    pub origin: Vec3,
    pub plane_normal: Vec3,
    pub start_s: f32,
}

pub fn gizmo_scale(camera: &Camera, origin: Vec3) -> f32 {
    ((camera.eye - origin).length() * 0.14).clamp(1.1, 9.0)
}

pub fn pick_gizmo_axis(
    camera: &Camera,
    origin: Vec3,
    ray_orig: Vec3,
    ray_dir: Vec3,
) -> Option<GizmoAxis> {
    let scale = gizmo_scale(camera, origin);
    let hit_radius = 0.08 * scale;
    let start_offset = 0.05 * scale;
    let seg_len = (1.04 - 0.05) * scale;

    let mut best: Option<(f32, GizmoAxis)> = None;
    for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
        let dir = axis.unit_dir();
        let seg_a = origin + dir * start_offset;
        let dist = ray_segment_distance(ray_orig, ray_dir, seg_a, dir, seg_len);
        if dist < hit_radius && best.map_or(true, |(bd, _)| dist < bd) {
            best = Some((dist, axis));
        }
    }
    best.map(|(_, axis)| axis)
}

pub fn compute_drag_plane_normal(camera: &Camera, origin: Vec3, axis: GizmoAxis) -> Vec3 {
    let axis_dir = axis.unit_dir();
    let view_dir = (camera.eye - origin).normalize_or_zero();
    let mut n = axis_dir.cross(view_dir.cross(axis_dir));
    if n.length_squared() < 1e-4 {
        n = axis_dir.cross(Vec3::Y);
        if n.length_squared() < 1e-4 {
            n = axis_dir.cross(Vec3::X);
        }
    }
    n.normalize()
}

pub fn project_ray_on_axis(
    ray_orig: Vec3,
    ray_dir: Vec3,
    origin: Vec3,
    axis_dir: Vec3,
    plane_normal: Vec3,
) -> Option<f32> {
    let denom = ray_dir.dot(plane_normal);
    if denom.abs() < 1e-4 {
        return None;
    }
    let t = (origin - ray_orig).dot(plane_normal) / denom;
    if t <= 0.0 {
        return None;
    }
    let hit = ray_orig + ray_dir * t;
    Some((hit - origin).dot(axis_dir))
}

fn ray_segment_distance(
    ray_orig: Vec3,
    ray_dir: Vec3,
    seg_a: Vec3,
    seg_dir: Vec3,
    seg_len: f32,
) -> f32 {
    ray_segment_closest(ray_orig, ray_dir, seg_a, seg_dir, seg_len).0
}

fn ray_segment_closest(
    ray_orig: Vec3,
    ray_dir: Vec3,
    seg_a: Vec3,
    seg_dir: Vec3,
    seg_len: f32,
) -> (f32, f32, Vec3) {
    let w0 = ray_orig - seg_a;
    let a = ray_dir.length_squared();
    let b = ray_dir.dot(seg_dir);
    let c = seg_dir.length_squared();
    let d = ray_dir.dot(w0);
    let e = seg_dir.dot(w0);
    let denom = a * c - b * b;

    let (t, u) = if denom > 1e-6 {
        let mut t = ((b * e - c * d) / denom).max(0.0);
        let mut u = (b * t + e) / c;
        if u < 0.0 {
            u = 0.0;
            t = (-d / a).max(0.0);
        } else if u > seg_len {
            u = seg_len;
            t = ((b * seg_len - d) / a).max(0.0);
        }
        (t, u)
    } else {
        let u = 0.5 * seg_len;
        let t = ((b * u - d) / a).max(0.0);
        (t, u)
    };

    let p_ray = ray_orig + ray_dir * t;
    let p_seg = seg_a + seg_dir * u;
    ((p_ray - p_seg).length(), t, p_seg)
}

fn axis_plane_basis(axis: GizmoAxis) -> (Vec3, Vec3) {
    match axis {
        GizmoAxis::X => (Vec3::Y, Vec3::Z),
        GizmoAxis::Y => (Vec3::Z, Vec3::X),
        GizmoAxis::Z => (Vec3::X, Vec3::Y),
    }
}

fn closest_point_on_ring_to_ray(
    origin: Vec3,
    ring_r: f32,
    axis: GizmoAxis,
    ray_orig: Vec3,
    ray_dir: Vec3,
) -> (f32, f32, Vec3) {
    let (u, v) = axis_plane_basis(axis);
    let mut best = (f32::MAX, f32::MAX, origin + u * ring_r);
    for i in 0..ROTATE_PICK_SEGMENTS {
        let a0 = (i as f32 / ROTATE_PICK_SEGMENTS as f32) * TAU;
        let a1 = ((i + 1) as f32 / ROTATE_PICK_SEGMENTS as f32) * TAU;
        let p0 = origin + (u * a0.cos() + v * a0.sin()) * ring_r;
        let p1 = origin + (u * a1.cos() + v * a1.sin()) * ring_r;
        let delta = p1 - p0;
        let seg_len = delta.length();
        let (dist, ray_t, p_seg) =
            ray_segment_closest(ray_orig, ray_dir, p0, delta / seg_len, seg_len);
        let better = dist + 1e-4 < best.0
            || ((dist - best.0).abs() <= 1e-4 && ray_t < best.1);
        if better {
            best = (dist, ray_t, p_seg);
        }
    }
    best
}

pub fn pick_rotate_gizmo_axis(
    camera: &Camera,
    origin: Vec3,
    ray_orig: Vec3,
    ray_dir: Vec3,
) -> Option<GizmoAxis> {
    let scale = gizmo_scale(camera, origin);
    let ring_r = ROTATE_RING_RADIUS * scale;
    let hit_radius = 0.085 * scale;
    let tie_window = 0.02 * scale;

    let mut best: Option<(f32, f32, GizmoAxis)> = None;
    for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
        let (dist, ray_t, _) =
            closest_point_on_ring_to_ray(origin, ring_r, axis, ray_orig, ray_dir);
        if dist < hit_radius {
            let take = match best {
                None => true,
                Some((bd, bt, _)) => {
                    if (dist - bd).abs() <= tie_window {
                        ray_t < bt
                    } else {
                        dist < bd
                    }
                }
            };
            if take {
                best = Some((dist, ray_t, axis));
            }
        }
    }
    best.map(|(_, _, axis)| axis)
}

#[derive(Debug, Clone, Copy)]
pub struct RotateDragState {
    pub axis: GizmoAxis,
    pub origin: Vec3,
    pub start_rot: [f32; 3],
    pub last_angle: f32,
    pub accum_angle: f32,
    pub use_screen_fallback: bool,
    pub screen_tan: Vec2,
    pub last_mouse_px: Vec2,
}

fn ray_plane_ring_angle(
    ray_orig: Vec3,
    ray_dir: Vec3,
    origin: Vec3,
    axis: GizmoAxis,
) -> Option<f32> {
    let n = axis.unit_dir();
    let denom = ray_dir.dot(n);
    if denom.abs() < 1e-4 {
        return None;
    }
    let t = (origin - ray_orig).dot(n) / denom;
    if t <= 0.0 {
        return None;
    }
    let radial = (ray_orig + ray_dir * t) - origin;
    if radial.length_squared() < 1e-4 {
        return None;
    }
    let (u, v) = axis_plane_basis(axis);
    Some(radial.dot(v).atan2(radial.dot(u)))
}

#[allow(clippy::too_many_arguments)]
pub fn begin_rotate_drag(
    camera: &Camera,
    origin: Vec3,
    axis: GizmoAxis,
    ray_orig: Vec3,
    ray_dir: Vec3,
    mouse_px: Vec2,
    view_w: f32,
    view_h: f32,
    start_rot: [f32; 3],
) -> RotateDragState {
    let scale = gizmo_scale(camera, origin);
    let grazing = ray_dir.dot(axis.unit_dir()).abs() < 0.12;
    let start_angle = if !grazing {
        ray_plane_ring_angle(ray_orig, ray_dir, origin, axis)
    } else {
        None
    };

    if let Some(ang) = start_angle {
        RotateDragState {
            axis,
            origin,
            start_rot,
            last_angle: ang,
            accum_angle: 0.0,
            use_screen_fallback: false,
            screen_tan: Vec2::X,
            last_mouse_px: mouse_px,
        }
    } else {
        let (_, _, hit_pt) = closest_point_on_ring_to_ray(
            origin,
            ROTATE_RING_RADIUS * scale,
            axis,
            ray_orig,
            ray_dir,
        );
        let tan3d = axis
            .unit_dir()
            .cross(hit_pt - origin)
            .normalize_or_zero();
        let screen_tan = match (
            camera.world_to_screen(hit_pt, view_w, view_h),
            camera.world_to_screen(hit_pt + tan3d * (0.25 * scale), view_w, view_h),
        ) {
            (Some(s0), Some(s1)) => (s1 - s0).normalize_or_zero(),
            _ => Vec2::X,
        };
        let screen_tan = if screen_tan.length_squared() < 1e-4 {
            Vec2::X
        } else {
            screen_tan
        };
        RotateDragState {
            axis,
            origin,
            start_rot,
            last_angle: 0.0,
            accum_angle: 0.0,
            use_screen_fallback: true,
            screen_tan,
            last_mouse_px: mouse_px,
        }
    }
}

pub fn update_rotate_drag(
    drag: &mut RotateDragState,
    ray_orig: Vec3,
    ray_dir: Vec3,
    mouse_px: Vec2,
) -> Option<[f32; 3]> {
    if drag.use_screen_fallback {
        let px_delta = mouse_px - drag.last_mouse_px;
        drag.last_mouse_px = mouse_px;
        let delta = px_delta.dot(drag.screen_tan) * 0.015;
        if delta.abs() <= 1e-5 {
            return None;
        }
        drag.accum_angle += delta;
    } else {
        let new_angle = ray_plane_ring_angle(ray_orig, ray_dir, drag.origin, drag.axis)?;
        let mut delta = new_angle - drag.last_angle;
        if delta > PI {
            delta -= TAU;
        } else if delta < -PI {
            delta += TAU;
        }
        drag.last_angle = new_angle;
        drag.accum_angle += delta;
    }

    let start_quat = Quat::from_euler(
        EulerRot::YXZ,
        drag.start_rot[1],
        drag.start_rot[0],
        drag.start_rot[2],
    );
    let next_quat = Quat::from_axis_angle(drag.axis.unit_dir(), drag.accum_angle) * start_quat;
    let (ry, rx, rz) = next_quat.to_euler(EulerRot::YXZ);
    Some([rx.rem_euclid(TAU), ry.rem_euclid(TAU), rz.rem_euclid(TAU)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_camera() -> Camera {
        Camera {
            eye: Vec3::new(20.0, 18.0, 20.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            aspect: 16.0 / 9.0,
            fovy_deg: 45.0,
            znear: 0.25,
            zfar: 600.0,
        }
    }

    #[test]
    fn picks_each_gizmo_axis_and_rejects_miss() {
        let cam = test_camera();
        let origin = Vec3::ZERO;
        let scale = gizmo_scale(&cam, origin);

        for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
            let target_pt = origin + axis.unit_dir() * (0.65 * scale);
            let dir = (target_pt - cam.eye).normalize();
            assert_eq!(pick_gizmo_axis(&cam, origin, cam.eye, dir), Some(axis));
        }

        let miss_pt = Vec3::new(-4.0 * scale, 3.0 * scale, -4.0 * scale);
        let miss_dir = (miss_pt - cam.eye).normalize();
        assert_eq!(pick_gizmo_axis(&cam, origin, cam.eye, miss_dir), None);
    }

    #[test]
    fn projects_ray_along_axes_accurately() {
        let cam = test_camera();
        let origin = Vec3::new(10.0, 2.0, 12.0);

        for axis in [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z] {
            let n = compute_drag_plane_normal(&cam, origin, axis);
            let dir0 = (origin - cam.eye).normalize();
            let s0 = project_ray_on_axis(cam.eye, dir0, origin, axis.unit_dir(), n)
                .expect("origin ray must hit drag plane");
            assert!(s0.abs() < 1e-4, "expected 0 start projection, got {s0}");

            let shifted = origin + axis.unit_dir() * 3.25;
            let dir1 = (shifted - cam.eye).normalize();
            let s1 = project_ray_on_axis(cam.eye, dir1, origin, axis.unit_dir(), n)
                .expect("shifted ray must hit drag plane");
            assert!((s1 - 3.25).abs() < 1e-3, "expected 3.25 displacement, got {s1}");

            let neg = origin - axis.unit_dir() * 1.75;
            let dir2 = (neg - cam.eye).normalize();
            let s2 = project_ray_on_axis(cam.eye, dir2, origin, axis.unit_dir(), n)
                .expect("negative shifted ray must hit drag plane");
            assert!((s2 + 1.75).abs() < 1e-3, "expected -1.75 displacement, got {s2}");
        }
    }

    #[test]
    fn picks_each_rotate_ring_and_rejects_miss() {
        let cam = test_camera();
        let origin = Vec3::ZERO;
        let scale = gizmo_scale(&cam, origin);
        let r = ROTATE_RING_RADIUS * scale;

        // Pick a point on the front/top arc of each ring where the three rings are well separated.
        let samples = [
            (GizmoAxis::X, Vec3::new(0.0, r, 0.0)),
            (GizmoAxis::Y, Vec3::new(r, 0.0, 0.0)),
            (GizmoAxis::Z, Vec3::new(r, 0.0, 0.0)), // X=r, Y=0 is on both Y and Z; use 45 deg on Z instead:
        ];
        let _ = samples;
        let targets = [
            (GizmoAxis::X, Vec3::new(0.0, r * 0.7071, r * 0.7071)),
            (GizmoAxis::Y, Vec3::new(r * 0.7071, 0.0, r * 0.7071)),
            (GizmoAxis::Z, Vec3::new(r * 0.7071, r * 0.7071, 0.0)),
        ];
        for (axis, pt) in targets {
            let dir = (pt - cam.eye).normalize();
            assert_eq!(pick_rotate_gizmo_axis(&cam, origin, cam.eye, dir), Some(axis));
        }

        let miss_dir = (Vec3::new(-4.0 * scale, 4.0 * scale, -4.0 * scale) - cam.eye).normalize();
        assert_eq!(pick_rotate_gizmo_axis(&cam, origin, cam.eye, miss_dir), None);
    }

    #[test]
    fn rotate_drag_rotates_exact_world_angle() {
        let cam = test_camera();
        let origin = Vec3::ZERO;
        let scale = gizmo_scale(&cam, origin);
        let r = ROTATE_RING_RADIUS * scale;

        // Drag X ring from +Y to +Z (+90 deg around +X)
        let p0 = Vec3::new(0.0, r, 0.0);
        let dir0 = (p0 - cam.eye).normalize();
        let mut drag = begin_rotate_drag(
            &cam,
            origin,
            GizmoAxis::X,
            cam.eye,
            dir0,
            Vec2::ZERO,
            1280.0,
            720.0,
            [0.0, 0.0, 0.0],
        );
        let p1 = Vec3::new(0.0, 0.0, r);
        let dir1 = (p1 - cam.eye).normalize();
        let next = update_rotate_drag(&mut drag, cam.eye, dir1, Vec2::ZERO)
            .expect("drag update should produce rotation");
        let expected_x = std::f32::consts::FRAC_PI_2;
        assert!(
            (next[0] - expected_x).abs() < 1e-3,
            "expected X rot ~ {expected_x}, got {:?}",
            next
        );
        assert!(next[1].abs() < 1e-3 || (next[1] - TAU).abs() < 1e-3);
        assert!(next[2].abs() < 1e-3 || (next[2] - TAU).abs() < 1e-3);
    }
}

#![cfg_attr(not(feature = "editor"), allow(dead_code))]

use glam::Vec3;
use glow::HasContext;
use shared::world::{RoadSpline, build_road_stations};

pub struct RoadMeshGpu {
    pub(super) vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    pub(super) index_count: i32,
}

impl RoadMeshGpu {
    pub fn from_road(
        gl: &glow::Context,
        road: &RoadSpline,
        start_away_dir: Option<[f32; 2]>,
        end_away_dir: Option<[f32; 2]>,
        overlap_tier: u8,
        is_soft_edge: bool,
    ) -> Result<Self, String> {
        let (verts, indices) =
            build_road_ribbon(road, start_away_dir, end_away_dir, overlap_tier, is_soft_edge);

        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let v_bytes: &[u8] =
                std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, v_bytes, glow::STATIC_DRAW);

            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            let i_bytes: &[u8] =
                std::slice::from_raw_parts(indices.as_ptr() as *const u8, indices.len() * 4);
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, i_bytes, glow::STATIC_DRAW);

            let stride = 8 * 4;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 24);
            gl.bind_vertex_array(None);

            Ok(Self {
                vao,
                vbo,
                ebo,
                index_count: indices.len() as i32,
            })
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_buffer(self.ebo);
        }
    }
}

fn push_vert(verts: &mut Vec<f32>, pos: [f32; 3], normal: [f32; 3], uv: [f32; 2]) {
    verts.extend_from_slice(&[
        pos[0], pos[1], pos[2], normal[0], normal[1], normal[2], uv[0], uv[1],
    ]);
}

/// Builds interleaved 8-float vertices (`[pos3, normal3, uv2]`) and `u32` triangle indices for a road ribbon.
pub fn build_road_ribbon(
    road: &RoadSpline,
    start_away_dir: Option<[f32; 2]>,
    end_away_dir: Option<[f32; 2]>,
    overlap_tier: u8,
    is_soft_edge: bool,
) -> (Vec<f32>, Vec<u32>) {
    let stations = build_road_stations(road, start_away_dir, end_away_dir);
    if stations.len() < 2 {
        return (Vec::new(), Vec::new());
    }

    let is_dirt = is_soft_edge;
    let half_w = (road.width * 0.5).max(0.5);
    let lift = if is_dirt { 0.032 } else { 0.045 } + overlap_tier as f32 * 0.0006;
    let skirt_w = half_w + 0.06;
    let skirt_dy = -0.14f32;
    let v_scale = 0.25f32;

    let mut verts: Vec<f32> = Vec::with_capacity(stations.len() * 32);
    let mut indices: Vec<u32> = Vec::with_capacity(stations.len() * 18);

    // Smooth vertex normals using tangent tilt along grade, keeping cross-section flat.
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(stations.len());
    for i in 0..stations.len() {
        let i0 = i.saturating_sub(1);
        let i1 = (i + 1).min(stations.len() - 1);
        let ds = (stations[i1].arc_len - stations[i0].arc_len).max(0.05);
        let dy = stations[i1].pos[1] - stations[i0].pos[1];
        let f = stations[i].forward_xz;
        let t3 = Vec3::new(f[0] * ds, dy, f[1] * ds);
        let r3 = Vec3::new(stations[i].right_xz[0], 0.0, stations[i].right_xz[1]);
        // right × forward = +Y in right-handed Y-up space.
        let n = r3.cross(t3).normalize_or_zero();
        normals.push(if n.y > 0.1 { n.to_array() } else { [0.0, 1.0, 0.0] });
    }

    let stride = if is_dirt { 2u32 } else { 4u32 };
    for (i, st) in stations.iter().enumerate() {
        let rx = st.right_xz[0] * st.miter_scale;
        let rz = st.right_xz[1] * st.miter_scale;
        let cy = st.pos[1] + lift;
        let v = st.arc_len * v_scale;
        let n = normals[i];

        let lc = [st.pos[0] - rx * half_w, cy, st.pos[2] - rz * half_w];
        let rc = [st.pos[0] + rx * half_w, cy, st.pos[2] + rz * half_w];

        if is_dirt {
            push_vert(&mut verts, lc, n, [0.0, v]);
            push_vert(&mut verts, rc, n, [1.0, v]);
        } else {
            let ls = [st.pos[0] - rx * skirt_w, cy + skirt_dy, st.pos[2] - rz * skirt_w];
            let rs = [st.pos[0] + rx * skirt_w, cy + skirt_dy, st.pos[2] + rz * skirt_w];
            let ln = Vec3::new(-st.right_xz[0], 0.35, -st.right_xz[1])
                .normalize_or_zero()
                .to_array();
            let rn = Vec3::new(st.right_xz[0], 0.35, st.right_xz[1])
                .normalize_or_zero()
                .to_array();
            push_vert(&mut verts, ls, ln, [0.0, v]);
            push_vert(&mut verts, lc, n, [0.0, v]);
            push_vert(&mut verts, rc, n, [1.0, v]);
            push_vert(&mut verts, rs, rn, [1.0, v]);
        }
    }

    for i in 0..(stations.len() as u32 - 1) {
        let b0 = i * stride;
        let b1 = (i + 1) * stride;
        if is_dirt {
            indices.extend_from_slice(&[b0, b0 + 1, b1, b0 + 1, b1 + 1, b1]);
        } else {
            // Left skirt (LS0, LC0, RC0, RS0 -> 0, 1, 2, 3)
            indices.extend_from_slice(&[b0, b0 + 1, b1, b0 + 1, b1 + 1, b1]);
            // Top deck
            indices.extend_from_slice(&[b0 + 1, b0 + 2, b1 + 1, b0 + 2, b1 + 2, b1 + 1]);
            // Right skirt
            indices.extend_from_slice(&[b0 + 2, b0 + 3, b1 + 2, b0 + 3, b1 + 3, b1 + 2]);
        }
    }

    const CAP_SEGS: usize = 8;
    // Start semicircular endcap when start endpoint does not miter into another road.
    if start_away_dir.is_none() {
        let st = &stations[0];
        let n = normals[0];
        let cy = st.pos[1] + lift;
        let base_v = st.arc_len * v_scale;
        let center_idx = (verts.len() / 8) as u32;
        push_vert(&mut verts, [st.pos[0], cy, st.pos[2]], n, [0.5, base_v]);

        let mut rim = Vec::with_capacity(CAP_SEGS + 1);
        let mut skirt = Vec::with_capacity(CAP_SEGS + 1);
        for s in 0..=CAP_SEGS {
            let theta = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::PI * (s as f32 / CAP_SEGS as f32);
            let dir_x = st.right_xz[0] * theta.sin() - st.forward_xz[0] * theta.cos();
            let dir_z = st.right_xz[1] * theta.sin() - st.forward_xz[1] * theta.cos();
            let rx = dir_x * half_w;
            let rz = dir_z * half_w;
            let u = 0.5 + 0.5 * theta.sin();
            let v = base_v - theta.cos() * half_w * v_scale;

            let r_idx = (verts.len() / 8) as u32;
            push_vert(&mut verts, [st.pos[0] + rx, cy, st.pos[2] + rz], n, [u, v]);
            rim.push(r_idx);

            if !is_dirt {
                let s_idx = (verts.len() / 8) as u32;
                let sn = Vec3::new(dir_x, 0.35, dir_z).normalize_or_zero().to_array();
                push_vert(
                    &mut verts,
                    [st.pos[0] + dir_x * skirt_w, cy + skirt_dy, st.pos[2] + dir_z * skirt_w],
                    sn,
                    [u, v],
                );
                skirt.push(s_idx);
            }
        }
        for s in 0..CAP_SEGS {
            indices.extend_from_slice(&[center_idx, rim[s], rim[s + 1]]);
            if !is_dirt {
                indices.extend_from_slice(&[
                    skirt[s],
                    skirt[s + 1],
                    rim[s],
                    rim[s],
                    skirt[s + 1],
                    rim[s + 1],
                ]);
            }
        }
    }

    // End semicircular endcap when end endpoint does not miter into another road.
    if end_away_dir.is_none() {
        let last = stations.len() - 1;
        let st = &stations[last];
        let n = normals[last];
        let cy = st.pos[1] + lift;
        let base_v = st.arc_len * v_scale;
        let center_idx = (verts.len() / 8) as u32;
        push_vert(&mut verts, [st.pos[0], cy, st.pos[2]], n, [0.5, base_v]);

        let mut rim = Vec::with_capacity(CAP_SEGS + 1);
        let mut skirt = Vec::with_capacity(CAP_SEGS + 1);
        for s in 0..=CAP_SEGS {
            let theta = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::PI * (s as f32 / CAP_SEGS as f32);
            let dir_x = st.right_xz[0] * theta.sin() + st.forward_xz[0] * theta.cos();
            let dir_z = st.right_xz[1] * theta.sin() + st.forward_xz[1] * theta.cos();
            let rx = dir_x * half_w;
            let rz = dir_z * half_w;
            let u = 0.5 + 0.5 * theta.sin();
            let v = base_v + theta.cos() * half_w * v_scale;

            let r_idx = (verts.len() / 8) as u32;
            push_vert(&mut verts, [st.pos[0] + rx, cy, st.pos[2] + rz], n, [u, v]);
            rim.push(r_idx);

            if !is_dirt {
                let s_idx = (verts.len() / 8) as u32;
                let sn = Vec3::new(dir_x, 0.35, dir_z).normalize_or_zero().to_array();
                push_vert(
                    &mut verts,
                    [st.pos[0] + dir_x * skirt_w, cy + skirt_dy, st.pos[2] + dir_z * skirt_w],
                    sn,
                    [u, v],
                );
                skirt.push(s_idx);
            }
        }
        for s in 0..CAP_SEGS {
            indices.extend_from_slice(&[center_idx, rim[s + 1], rim[s]]);
            if !is_dirt {
                indices.extend_from_slice(&[
                    skirt[s],
                    rim[s],
                    skirt[s + 1],
                    rim[s],
                    rim[s + 1],
                    skirt[s + 1],
                ]);
            }
        }
    }

    (verts, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::world::{ROAD_MAT_BRICK, ROAD_MAT_DIRT, RoadStyle};

    #[test]
    fn ribbon_triangles_all_face_upward_or_outward() {
        for (style, mat) in [
            (RoadStyle::Curve, ROAD_MAT_BRICK),
            (RoadStyle::Straight, ROAD_MAT_DIRT),
        ] {
            let road = RoadSpline {
                id: 1,
                style,
                material: mat,
                width: 4.0,
                shoulder: 3.0,
                points: vec![[8.0, 0.0, 12.0], [20.0, 1.0, 16.0], [32.0, 2.0, 28.0]],
                profile_heights: Vec::new(),
            };
            let (verts, indices) =
                build_road_ribbon(&road, None, None, 0, mat == ROAD_MAT_DIRT);
            assert!(!verts.is_empty());
            assert!(!indices.is_empty());
            assert_eq!(indices.len() % 3, 0);

            for tri in indices.chunks_exact(3) {
                let p = |idx: u32| -> Vec3 {
                    let b = idx as usize * 8;
                    Vec3::new(verts[b], verts[b + 1], verts[b + 2])
                };
                let cross = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                assert!(
                    cross.y > 0.0,
                    "inverted triangle in {style:?}/{mat}: cross={cross:?}"
                );
            }
        }
    }
}

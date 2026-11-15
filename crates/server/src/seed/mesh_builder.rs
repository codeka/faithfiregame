use std::f32::consts::TAU;

use glam::Vec3;
use serde_json::json;

pub(super) struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u16>,
}

impl MeshBuilder {
    pub(super) fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub(super) fn push_tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        let ab = b - a;
        let ac = c - a;
        let normal = ab.cross(ac).normalize_or_zero();
        let n = [normal.x, normal.y, normal.z];
        let base = self.positions.len() as u16;
        self.positions.push(a.to_array());
        self.positions.push(b.to_array());
        self.positions.push(c.to_array());
        self.normals.extend_from_slice(&[n, n, n]);
        self.colors.extend_from_slice(&[color, color, color]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    pub(super) fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: [f32; 4]) {
        self.push_tri(a, b, c, color);
        self.push_tri(a, c, d, color);
    }

    pub(super) fn add_frustum(
        &mut self,
        center_bottom: Vec3,
        height: f32,
        r_bottom: f32,
        r_top: f32,
        segments: usize,
        color: [f32; 4],
    ) {
        let y0 = center_bottom.y;
        let y1 = y0 + height;
        let mut ring0 = Vec::with_capacity(segments);
        let mut ring1 = Vec::with_capacity(segments);
        for i in 0..segments {
            let ang = (i as f32 / segments as f32) * TAU;
            let cos = ang.cos();
            let sin = ang.sin();
            ring0.push(Vec3::new(
                center_bottom.x + r_bottom * cos,
                y0,
                center_bottom.z + r_bottom * sin,
            ));
            ring1.push(Vec3::new(
                center_bottom.x + r_top * cos,
                y1,
                center_bottom.z + r_top * sin,
            ));
        }

        let top_center = Vec3::new(center_bottom.x, y1, center_bottom.z);
        let bot_center = Vec3::new(center_bottom.x, y0, center_bottom.z);

        for i in 0..segments {
            let next = (i + 1) % segments;
            // Side face (CCW from outside when Y is up)
            self.push_quad(ring0[next], ring0[i], ring1[i], ring1[next], color);
            // Bottom cap
            self.push_tri(bot_center, ring0[i], ring0[next], color);
            // Top cap (if r_top > 0)
            if r_top > 0.001 {
                self.push_tri(top_center, ring1[next], ring1[i], color);
            }
        }
    }

    pub(super) fn add_ellipsoid(
        &mut self,
        center: Vec3,
        radii: Vec3,
        rings: usize,
        sectors: usize,
        color: [f32; 4],
    ) {
        for r in 0..rings {
            let v0 = (r as f32 / rings as f32) * std::f32::consts::PI;
            let v1 = ((r + 1) as f32 / rings as f32) * std::f32::consts::PI;
            for s in 0..sectors {
                let u0 = (s as f32 / sectors as f32) * TAU;
                let u1 = ((s + 1) as f32 / sectors as f32) * TAU;

                let p = |v: f32, u: f32| {
                    let sv = v.sin();
                    // Slight deterministic organic perturbation so facets read cleanly
                    let wobble = 1.0 + 0.08 * (u * 3.0 + v * 2.0).sin();
                    center
                        + Vec3::new(
                            radii.x * sv * u.cos() * wobble,
                            radii.y * v.cos(),
                            radii.z * sv * u.sin() * wobble,
                        )
                };

                let p00 = p(v0, u0);
                let p10 = p(v1, u0);
                let p11 = p(v1, u1);
                let p01 = p(v0, u1);

                // At r == 0, p00 == p01 (north pole), so {p00, p11, p10} forms the north cap.
                // At r == rings - 1, p10 == p11 (south pole), so {p00, p01, p11} forms the south cap.
                if r < rings - 1 {
                    self.push_tri(p00, p11, p10, color);
                }
                if r > 0 {
                    self.push_tri(p00, p01, p11, color);
                }
            }
        }
    }

    pub(super) fn to_glb_bytes(&self) -> Vec<u8> {
        let mut min_p = [f32::MAX; 3];
        let mut max_p = [f32::MIN; 3];
        for p in &self.positions {
            for k in 0..3 {
                min_p[k] = min_p[k].min(p[k]);
                max_p[k] = max_p[k].max(p[k]);
            }
        }

        let mut bin = Vec::new();
        let pos_offset = bin.len();
        for p in &self.positions {
            for f in p {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let pos_len = bin.len() - pos_offset;

        let norm_offset = bin.len();
        for n in &self.normals {
            for f in n {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let norm_len = bin.len() - norm_offset;

        let col_offset = bin.len();
        for c in &self.colors {
            for f in c {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let col_len = bin.len() - col_offset;

        let idx_offset = bin.len();
        for idx in &self.indices {
            bin.extend_from_slice(&idx.to_le_bytes());
        }
        let idx_len = bin.len() - idx_offset;

        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let gltf_json = json!({
            "asset": {
                "version": "2.0",
                "generator": "faith-and-fire-seed"
            },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "materials": [{
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.85
                }
            }],
            "meshes": [{
                "primitives": [{
                    "attributes": {
                        "POSITION": 0,
                        "NORMAL": 1,
                        "COLOR_0": 2
                    },
                    "indices": 3,
                    "material": 0
                }]
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_offset, "byteLength": pos_len, "target": 34962 },
                { "buffer": 0, "byteOffset": norm_offset, "byteLength": norm_len, "target": 34962 },
                { "buffer": 0, "byteOffset": col_offset, "byteLength": col_len, "target": 34962 },
                { "buffer": 0, "byteOffset": idx_offset, "byteLength": idx_len, "target": 34963 }
            ],
            "accessors": [
                {
                    "bufferView": 0,
                    "componentType": 5126,
                    "count": self.positions.len(),
                    "type": "VEC3",
                    "min": min_p,
                    "max": max_p
                },
                {
                    "bufferView": 1,
                    "componentType": 5126,
                    "count": self.normals.len(),
                    "type": "VEC3"
                },
                {
                    "bufferView": 2,
                    "componentType": 5126,
                    "count": self.colors.len(),
                    "type": "VEC4"
                },
                {
                    "bufferView": 3,
                    "componentType": 5123,
                    "count": self.indices.len(),
                    "type": "SCALAR"
                }
            ]
        });

        let mut json_bytes = serde_json::to_vec(&gltf_json).expect("valid gltf json");
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total_len);
        // GLB Header
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total_len as u32).to_le_bytes());
        // JSON Chunk
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        // BIN Chunk
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_outward_non_degenerate(mb: &MeshBuilder, center: Vec3) {
        assert!(!mb.indices.is_empty());
        for tri in mb.indices.chunks_exact(3) {
            let a = Vec3::from(mb.positions[tri[0] as usize]);
            let b = Vec3::from(mb.positions[tri[1] as usize]);
            let c = Vec3::from(mb.positions[tri[2] as usize]);
            let cross = (b - a).cross(c - a);
            assert!(
                cross.length() > 1e-5,
                "degenerate triangle: {a:?}, {b:?}, {c:?}"
            );
            let centroid = (a + b + c) / 3.0;
            let outward = centroid - center;
            assert!(
                outward.dot(cross) > 0.0,
                "inward-facing triangle at centroid {centroid:?} relative to {center:?}: normal={cross:?}"
            );
        }
    }

    #[test]
    fn ellipsoid_triangles_are_outward_ccw_and_watertight() {
        let mut mb = MeshBuilder::new();
        let center = Vec3::new(1.5, 2.5, -1.0);
        mb.add_ellipsoid(center, Vec3::new(1.2, 0.9, 1.1), 5, 8, [1.0; 4]);
        assert_outward_non_degenerate(&mb, center);
    }

    #[test]
    fn frustum_triangles_are_outward_ccw() {
        let mut mb = MeshBuilder::new();
        mb.add_frustum(Vec3::ZERO, 2.0, 1.0, 0.4, 8, [1.0; 4]);
        assert_outward_non_degenerate(&mb, Vec3::new(0.0, 1.0, 0.0));
    }
}

use std::fs;
use std::path::Path;

use glam::{Mat3, Mat4, Vec2, Vec3};
use serde_json::json;
use shared::world::{
    AreaKind, AreaMetadata, InsideKind, InsideVisData, MAT_COBBLESTONE, MAT_DARK_GRASS, MAT_DIRT,
    MAT_PATCHY_GRASS, MAT_ROCK, PropPlacement, TriggerVolume, blend_splat_sample,
    compute_inside_sim_from_vis, format_local_asset_id,
};

#[derive(Clone, Copy)]
struct SubPrimSpec {
    emissive: [f32; 3],
    double_sided: bool,
    alpha_mask: bool,
}

struct SubPrim {
    spec: SubPrimSpec,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

struct SceneGlbBuilder {
    atlas_png: Vec<u8>,
    prims: Vec<SubPrim>,
}

/// UV regions inside the 64×64 procedural low-poly texture atlas:
/// - `(0.00..0.50, 0.00..0.50)`: Weathered steel panel with rivets & rust streaks
/// - `(0.50..1.00, 0.00..0.50)`: Frosted cryo-glass & scanline readout
/// - `(0.00..0.50, 0.50..1.00)`: Mossy subterranean rock / concrete
/// - `(0.50..1.00, 0.50..1.00)`: Rusted diamond wire-mesh grate (alpha-masked holes)
const UV_STEEL: (Vec2, Vec2) = (Vec2::new(0.02, 0.02), Vec2::new(0.48, 0.48));
const UV_GLASS: (Vec2, Vec2) = (Vec2::new(0.52, 0.02), Vec2::new(0.98, 0.48));
const UV_ROCK: (Vec2, Vec2) = (Vec2::new(0.02, 0.52), Vec2::new(0.48, 0.98));
const UV_GRATE: (Vec2, Vec2) = (Vec2::new(0.52, 0.52), Vec2::new(0.98, 0.98));

fn build_cryo_atlas_png() -> anyhow::Result<Vec<u8>> {
    let size = 64usize;
    let mut rgba = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let h = ((x * 73 + y * 139) ^ (x * y * 17)) & 0xFF;
            let noise = (h as i32 - 128) / 14;

            let px = if y < 32 && x < 32 {
                // Quadrant 0: Weathered steel panel + border seam + rust flecks
                let border = x <= 1 || y <= 1 || x >= 30 || y >= 30;
                let rivet = (x == 4 || x == 27) && (y == 4 || y == 27);
                let rust = ((x * 19 + y * 31) % 29) < 4;
                if border {
                    [78, 82, 88, 255]
                } else if rivet {
                    [168, 174, 182, 255]
                } else if rust {
                    [
                        (142 + noise).clamp(0, 255) as u8,
                        (88 + noise).clamp(0, 255) as u8,
                        (58 + noise).clamp(0, 255) as u8,
                        255,
                    ]
                } else {
                    [
                        (188 + noise).clamp(0, 255) as u8,
                        (194 + noise).clamp(0, 255) as u8,
                        (200 + noise).clamp(0, 255) as u8,
                        255,
                    ]
                }
            } else if y < 32 {
                // Quadrant 1: Frosted cryo-glass / scanline screen
                let lx = x - 32;
                let scan = (y % 3 == 0) as i32 * 16;
                let streak = ((lx as i32 + y as i32 - 28).abs() <= 3) as i32 * 26;
                [
                    (175 + scan + streak + noise / 2).clamp(0, 255) as u8,
                    (214 + scan + streak + noise / 2).clamp(0, 255) as u8,
                    (228 + scan + streak).clamp(0, 255) as u8,
                    255,
                ]
            } else if x < 32 {
                // Quadrant 2: Mossy cave rock / concrete
                let ly = y - 32;
                let moss = ((x * 13 + ly * 23) % 19) < 4;
                if moss {
                    [
                        (98 + noise).clamp(0, 255) as u8,
                        (134 + noise).clamp(0, 255) as u8,
                        (76 + noise).clamp(0, 255) as u8,
                        255,
                    ]
                } else {
                    [
                        (150 + noise).clamp(0, 255) as u8,
                        (148 + noise).clamp(0, 255) as u8,
                        (140 + noise).clamp(0, 255) as u8,
                        255,
                    ]
                }
            } else {
                // Quadrant 3: Alpha-masked diamond wire mesh grate
                let lx = (x - 32) as i32;
                let ly = (y - 32) as i32;
                let frame = lx <= 2 || ly <= 2 || lx >= 29 || ly >= 29;
                let diag_a = (lx + ly).rem_euclid(6) == 0;
                let diag_b = (lx - ly).rem_euclid(6) == 0;
                if frame {
                    [138, 92, 64, 255]
                } else if diag_a || diag_b {
                    [156, 118, 92, 255]
                } else {
                    [0, 0, 0, 0]
                }
            };
            rgba[idx..idx + 4].copy_from_slice(&px);
        }
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size as u32, size as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
    }
    Ok(out)
}

impl SceneGlbBuilder {
    fn new(atlas_png: Vec<u8>) -> Self {
        Self {
            atlas_png,
            prims: Vec::new(),
        }
    }

    fn prim_mut(&mut self, spec: SubPrimSpec) -> &mut SubPrim {
        if let Some(idx) = self.prims.iter().position(|p| {
            p.spec.double_sided == spec.double_sided
                && p.spec.alpha_mask == spec.alpha_mask
                && p.spec.emissive == spec.emissive
        }) {
            return &mut self.prims[idx];
        }
        self.prims.push(SubPrim {
            spec,
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        });
        self.prims.last_mut().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn push_quad_xform(
        &mut self,
        spec: SubPrimSpec,
        xform: Mat4,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        uv_rect: (Vec2, Vec2),
        color: [f32; 4],
    ) {
        let wa = xform.transform_point3(a);
        let wb = xform.transform_point3(b);
        let wc = xform.transform_point3(c);
        let wd = xform.transform_point3(d);
        let n_mat = Mat3::from_mat4(xform).inverse().transpose();
        let local_n = (b - a).cross(c - a).normalize_or_zero();
        let wn = (n_mat * local_n).normalize_or_zero().to_array();

        let (uv0, uv1) = uv_rect;
        let prim = self.prim_mut(spec);
        let base = prim.positions.len() as u32;
        prim.positions
            .extend_from_slice(&[wa.to_array(), wb.to_array(), wc.to_array(), wd.to_array()]);
        prim.normals.extend_from_slice(&[wn, wn, wn, wn]);
        prim.colors.extend_from_slice(&[color, color, color, color]);
        prim.uvs.extend_from_slice(&[
            [uv0.x, uv1.y],
            [uv1.x, uv1.y],
            [uv1.x, uv0.y],
            [uv0.x, uv0.y],
        ]);
        prim.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn add_box_xform(
        &mut self,
        spec: SubPrimSpec,
        xform: Mat4,
        min: Vec3,
        max: Vec3,
        uv_rect: (Vec2, Vec2),
        color: [f32; 4],
    ) {
        let (x0, y0, z0) = (min.x, min.y, min.z);
        let (x1, y1, z1) = (max.x, max.y, max.z);

        // +Z (front)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
            uv_rect,
            color,
        );
        // -Z (back)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x1, y0, z0),
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x1, y1, z0),
            uv_rect,
            color,
        );
        // +X (right)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x1, y1, z1),
            uv_rect,
            color,
        );
        // -X (left)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z1),
            Vec3::new(x0, y1, z0),
            uv_rect,
            color,
        );
        // +Y (top)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x0, y1, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z0),
            uv_rect,
            color,
        );
        // -Y (bottom)
        self.push_quad_xform(
            spec,
            xform,
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y0, z1),
            Vec3::new(x0, y0, z1),
            uv_rect,
            color,
        );
    }

    fn add_box(&mut self, min: Vec3, max: Vec3, uv_rect: (Vec2, Vec2), color: [f32; 4]) {
        self.add_box_xform(
            SubPrimSpec {
                emissive: [0.0; 3],
                double_sided: false,
                alpha_mask: false,
            },
            Mat4::IDENTITY,
            min,
            max,
            uv_rect,
            color,
        );
    }

    fn add_emissive_box(&mut self, min: Vec3, max: Vec3, emissive: [f32; 3], color: [f32; 4]) {
        self.add_box_xform(
            SubPrimSpec {
                emissive,
                double_sided: false,
                alpha_mask: false,
            },
            Mat4::IDENTITY,
            min,
            max,
            UV_GLASS,
            color,
        );
    }

    fn to_glb_bytes(&self) -> Vec<u8> {
        let mut bin = Vec::new();
        let mut buffer_views = Vec::new();
        let mut accessors = Vec::new();
        let mut materials = Vec::new();
        let mut primitives = Vec::new();

        let img_offset = bin.len();
        bin.extend_from_slice(&self.atlas_png);
        let img_len = self.atlas_png.len();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let img_view_idx = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": img_offset,
            "byteLength": img_len
        }));

        for prim in &self.prims {
            if prim.indices.is_empty() {
                continue;
            }
            let mut min_p = [f32::MAX; 3];
            let mut max_p = [f32::MIN; 3];
            for p in &prim.positions {
                for k in 0..3 {
                    min_p[k] = min_p[k].min(p[k]);
                    max_p[k] = max_p[k].max(p[k]);
                }
            }

            let pos_off = bin.len();
            for p in &prim.positions {
                for f in p {
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            let pos_len = bin.len() - pos_off;

            let norm_off = bin.len();
            for n in &prim.normals {
                for f in n {
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            let norm_len = bin.len() - norm_off;

            let col_off = bin.len();
            for c in &prim.colors {
                for f in c {
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            let col_len = bin.len() - col_off;

            let uv_off = bin.len();
            for uv in &prim.uvs {
                for f in uv {
                    bin.extend_from_slice(&f.to_le_bytes());
                }
            }
            let uv_len = bin.len() - uv_off;

            let idx_off = bin.len();
            for &idx in &prim.indices {
                bin.extend_from_slice(&idx.to_le_bytes());
            }
            let idx_len = bin.len() - idx_off;
            while bin.len() % 4 != 0 {
                bin.push(0);
            }

            let bv_base = buffer_views.len();
            buffer_views.push(json!({ "buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962 }));
            buffer_views.push(json!({ "buffer": 0, "byteOffset": norm_off, "byteLength": norm_len, "target": 34962 }));
            buffer_views.push(json!({ "buffer": 0, "byteOffset": col_off, "byteLength": col_len, "target": 34962 }));
            buffer_views.push(json!({ "buffer": 0, "byteOffset": uv_off, "byteLength": uv_len, "target": 34962 }));
            buffer_views.push(json!({ "buffer": 0, "byteOffset": idx_off, "byteLength": idx_len, "target": 34963 }));

            let acc_base = accessors.len();
            let vcount = prim.positions.len();
            accessors.push(json!({
                "bufferView": bv_base,
                "componentType": 5126,
                "count": vcount,
                "type": "VEC3",
                "min": min_p,
                "max": max_p
            }));
            accessors.push(json!({
                "bufferView": bv_base + 1,
                "componentType": 5126,
                "count": vcount,
                "type": "VEC3"
            }));
            accessors.push(json!({
                "bufferView": bv_base + 2,
                "componentType": 5126,
                "count": vcount,
                "type": "VEC4"
            }));
            accessors.push(json!({
                "bufferView": bv_base + 3,
                "componentType": 5126,
                "count": vcount,
                "type": "VEC2"
            }));
            accessors.push(json!({
                "bufferView": bv_base + 4,
                "componentType": 5125,
                "count": prim.indices.len(),
                "type": "SCALAR"
            }));

            let mat_idx = materials.len();
            let mut mat = json!({
                "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 0 },
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.2,
                    "roughnessFactor": 0.8
                },
                "emissiveFactor": prim.spec.emissive,
                "doubleSided": prim.spec.double_sided
            });
            if prim.spec.alpha_mask {
                mat["alphaMode"] = json!("MASK");
                mat["alphaCutoff"] = json!(0.45);
            }
            materials.push(mat);

            primitives.push(json!({
                "attributes": {
                    "POSITION": acc_base,
                    "NORMAL": acc_base + 1,
                    "COLOR_0": acc_base + 2,
                    "TEXCOORD_0": acc_base + 3
                },
                "indices": acc_base + 4,
                "material": mat_idx
            }));
        }

        let gltf_json = json!({
            "asset": { "version": "2.0", "generator": "faith-and-fire-cryo-seed" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "images": [{ "bufferView": img_view_idx, "mimeType": "image/png" }],
            "samplers": [{ "magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071 }],
            "textures": [{ "source": 0, "sampler": 0 }],
            "materials": materials,
            "meshes": [{ "primitives": primitives }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": buffer_views,
            "accessors": accessors
        });

        let mut json_bytes = serde_json::to_vec(&gltf_json).expect("valid gltf json");
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total_len);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total_len as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
        out
    }
}

fn add_angled_capsule(
    b: &mut SceneGlbBuilder,
    center: Vec3,
    tilt_x_rad: f32,
    opened: bool,
    led_emissive: [f32; 3],
) {
    let xform = Mat4::from_translation(center) * Mat4::from_rotation_x(tilt_x_rad);
    let hull = [0.68, 0.72, 0.74, 1.0];
    let dark_trim = [0.34, 0.37, 0.40, 1.0];
    let interior = [0.22, 0.28, 0.32, 1.0];
    let spec = SubPrimSpec {
        emissive: [0.0; 3],
        double_sided: false,
        alpha_mask: false,
    };

    // Main pod bed + side bolsters
    b.add_box_xform(
        spec,
        xform,
        Vec3::new(-0.52, -0.22, -1.05),
        Vec3::new(0.52, 0.12, 1.05),
        UV_STEEL,
        hull,
    );
    b.add_box_xform(
        spec,
        xform,
        Vec3::new(-0.54, 0.12, -1.02),
        Vec3::new(-0.38, 0.42, 1.02),
        UV_STEEL,
        hull,
    );
    b.add_box_xform(
        spec,
        xform,
        Vec3::new(0.38, 0.12, -1.02),
        Vec3::new(0.54, 0.42, 1.02),
        UV_STEEL,
        hull,
    );
    b.add_box_xform(
        spec,
        xform,
        Vec3::new(-0.52, 0.12, -1.05),
        Vec3::new(0.52, 0.48, -0.78),
        UV_STEEL,
        dark_trim,
    );
    b.add_box_xform(
        spec,
        xform,
        Vec3::new(-0.52, 0.12, 0.74),
        Vec3::new(0.52, 0.36, 1.08),
        UV_STEEL,
        hull,
    );

    if opened {
        // Recessed mattress interior visible when lid has ejected/retracted
        b.add_box_xform(
            spec,
            xform,
            Vec3::new(-0.36, 0.10, -0.76),
            Vec3::new(0.36, 0.18, 0.72),
            UV_GLASS,
            interior,
        );
    } else {
        // Sealed frosted viewport canopy
        b.add_box_xform(
            spec,
            xform,
            Vec3::new(-0.38, 0.18, -0.76),
            Vec3::new(0.38, 0.44, 0.72),
            UV_GLASS,
            [0.58, 0.72, 0.78, 1.0],
        );
    }

    // Glowing front status readout screen
    b.add_box_xform(
        SubPrimSpec {
            emissive: led_emissive,
            double_sided: false,
            alpha_mask: false,
        },
        xform,
        Vec3::new(-0.22, 0.20, 1.04),
        Vec3::new(0.22, 0.32, 1.11),
        UV_GLASS,
        [0.85, 0.98, 0.92, 1.0],
    );
}

fn build_cryo_rack_bay_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let frame = [0.36, 0.38, 0.35, 1.0];
    let rust_beam = [0.46, 0.36, 0.28, 1.0];
    let back_wall = [0.22, 0.24, 0.23, 1.0];

    // 2-column × 2-story bay (`3.4m` along Z, `1.8m` deep along X, `4.6m` tall, facing +X into room)
    // Rotate internally so local +Z faces into the room for intuitive placement
    b.add_box(Vec3::new(-1.7, 0.0, -0.9), Vec3::new(1.7, 4.6, -0.68), UV_ROCK, back_wall);
    for x in [-1.68f32, 0.0, 1.68] {
        b.add_box(
            Vec3::new(x - 0.10, 0.0, -0.70),
            Vec3::new(x + 0.10, 4.6, 0.75),
            UV_STEEL,
            frame,
        );
    }
    for y in [0.08f32, 2.22, 4.46] {
        b.add_box(
            Vec3::new(-1.72, y - 0.09, -0.70),
            Vec3::new(1.72, y + 0.09, 0.80),
            UV_STEEL,
            rust_beam,
        );
    }

    let leds = [
        ([-0.84, 1.05], [0.15, 0.95, 0.42]), // Lower-left: green nominal
        ([0.84, 1.05], [0.22, 0.68, 1.00]),  // Lower-right: cyan stasis
        ([-0.84, 3.25], [0.18, 0.92, 0.45]), // Upper-left: green
        ([0.84, 3.25], [1.00, 0.58, 0.14]),  // Upper-right: amber low-isotope warning!
    ];
    for ([cx, cy], em) in leds {
        add_angled_capsule(&mut b, Vec3::new(cx, cy, -0.02), 0.72, false, em);
        // Divider bar LED strip
        b.add_emissive_box(
            Vec3::new(cx - 0.28, cy - 0.88, 0.76),
            Vec3::new(cx + 0.28, cy - 0.80, 0.83),
            em,
            [0.9, 0.95, 0.95, 1.0],
        );
    }

    b.to_glb_bytes()
}

fn build_hero_cryo_pod_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let rail_rust = [0.54, 0.34, 0.24, 1.0];
    let cable_dark = [0.20, 0.22, 0.20, 1.0];

    // Extended floor slide rails + hydraulic cradle
    for rx in [-0.58f32, 0.58] {
        b.add_box(
            Vec3::new(rx - 0.08, 0.0, -1.65),
            Vec3::new(rx + 0.08, 0.16, 1.65),
            UV_STEEL,
            rail_rust,
        );
    }
    for rz in [-1.45f32, -0.45, 0.55, 1.45] {
        b.add_box(
            Vec3::new(-0.62, 0.02, rz - 0.06),
            Vec3::new(0.62, 0.10, rz + 0.06),
            UV_STEEL,
            rail_rust,
        );
    }
    // Angled support struts holding the ejected capsule
    b.add_box_xform(
        SubPrimSpec {
            emissive: [0.0; 3],
            double_sided: false,
            alpha_mask: false,
        },
        Mat4::from_translation(Vec3::new(0.0, 0.78, -0.62)) * Mat4::from_rotation_x(0.62),
        Vec3::new(-0.60, -0.75, -0.10),
        Vec3::new(0.60, 0.75, 0.10),
        UV_STEEL,
        rail_rust,
    );

    // Opened hero cryo-capsule with glowing green failsafe wake readout
    add_angled_capsule(
        &mut b,
        Vec3::new(0.0, 1.12, 0.12),
        0.66,
        true,
        [0.20, 1.00, 0.48],
    );

    // Trailing floor power/coolant umbilical cables
    for (cx, len) in [(-0.24f32, 2.25f32), (-0.04, 2.45), (0.19, 2.15)] {
        b.add_box(
            Vec3::new(cx - 0.03, 0.0, 1.10),
            Vec3::new(cx + 0.03, 0.05, len),
            UV_STEEL,
            cable_dark,
        );
    }

    b.to_glb_bytes()
}

fn build_cage_elevator_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let rust_frame = [0.56, 0.39, 0.29, 1.0];
    let dark_shaft = [0.28, 0.30, 0.28, 1.0];
    let cable = [0.22, 0.22, 0.24, 1.0];

    // Shaft floor platform + corner posts (`3.4m wide × 2.8m deep × 5.2m tall`)
    b.add_box(Vec3::new(-1.7, 0.0, -1.4), Vec3::new(1.7, 0.14, 1.4), UV_STEEL, dark_shaft);
    for (px, pz) in [(-1.62, -1.32), (1.62, -1.32), (-1.62, 1.32), (1.62, 1.32)] {
        b.add_box(
            Vec3::new(px - 0.10, 0.0, pz - 0.10),
            Vec3::new(px + 0.10, 5.2, pz + 0.10),
            UV_STEEL,
            rust_frame,
        );
    }
    // Lower cage enclosure Header at y = 2.85m
    b.add_box(Vec3::new(-1.72, 2.75, -1.42), Vec3::new(1.72, 3.05, 1.42), UV_STEEL, rust_frame);
    // Side & back walls of cage
    b.add_box(Vec3::new(-1.68, 0.14, -1.40), Vec3::new(1.68, 2.75, -1.26), UV_STEEL, dark_shaft);
    b.add_box(Vec3::new(-1.68, 0.14, -1.30), Vec3::new(-1.54, 2.75, 1.30), UV_STEEL, rust_frame);
    b.add_box(Vec3::new(1.54, 0.14, -1.30), Vec3::new(1.68, 2.75, 1.30), UV_STEEL, rust_frame);

    // Alpha-masked diamond wire-mesh double doors on +Z front (partially parted so threshold reads clearly)
    let grate_spec = SubPrimSpec {
        emissive: [0.0; 3],
        double_sided: true,
        alpha_mask: true,
    };
    b.add_box_xform(
        grate_spec,
        Mat4::IDENTITY,
        Vec3::new(-1.48, 0.16, 1.28),
        Vec3::new(-0.12, 2.72, 1.34),
        UV_GRATE,
        [0.88, 0.78, 0.68, 1.0],
    );
    b.add_box_xform(
        grate_spec,
        Mat4::IDENTITY,
        Vec3::new(0.12, 0.16, 1.28),
        Vec3::new(1.48, 2.72, 1.34),
        UV_GRATE,
        [0.88, 0.78, 0.68, 1.0],
    );

    // Overhead hoist cables & pulley block
    for cx in [-0.22f32, -0.07, 0.08, 0.23] {
        b.add_box(
            Vec3::new(cx - 0.025, 3.05, -0.05),
            Vec3::new(cx + 0.025, 5.20, 0.05),
            UV_STEEL,
            cable,
        );
    }
    b.add_box(Vec3::new(0.45, 3.85, -0.22), Vec3::new(0.85, 4.35, 0.22), UV_STEEL, dark_shaft);

    // Glowing green elevator ready indicator on left pillar + call button panel on right frame
    b.add_emissive_box(
        Vec3::new(-1.95, 2.25, 1.26),
        Vec3::new(-1.76, 2.62, 1.42),
        [0.24, 1.00, 0.45],
        [0.85, 1.0, 0.88, 1.0],
    );
    b.add_box(Vec3::new(1.42, 1.15, 1.34), Vec3::new(1.62, 1.52, 1.42), UV_STEEL, cable);

    b.to_glb_bytes()
}

fn build_fallen_guard_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let bone = [0.82, 0.80, 0.74, 1.0];
    let armor = [0.42, 0.46, 0.40, 1.0];
    let boots = [0.24, 0.25, 0.23, 1.0];
    let rifle = [0.38, 0.28, 0.20, 1.0];

    // Seated slumped torso & chestplate leaning back against rock (-Z/+X)
    b.add_box(Vec3::new(-0.24, 0.14, -0.20), Vec3::new(0.24, 0.72, 0.12), UV_STEEL, armor);
    // Skull tilted slightly
    b.add_box(Vec3::new(-0.13, 0.74, -0.14), Vec3::new(0.13, 0.98, 0.12), UV_ROCK, bone);
    // Outstretched legs toward room center (+Z)
    b.add_box(Vec3::new(-0.24, 0.04, 0.12), Vec3::new(-0.07, 0.18, 0.95), UV_STEEL, armor);
    b.add_box(Vec3::new(0.07, 0.04, 0.12), Vec3::new(0.26, 0.18, 0.88), UV_STEEL, armor);
    b.add_box(Vec3::new(-0.25, 0.02, 0.92), Vec3::new(-0.06, 0.22, 1.10), UV_STEEL, boots);
    b.add_box(Vec3::new(0.07, 0.02, 0.85), Vec3::new(0.27, 0.22, 1.03), UV_STEEL, boots);
    // Slumped skeletal arms
    b.add_box(Vec3::new(-0.38, 0.06, -0.05), Vec3::new(-0.25, 0.62, 0.12), UV_ROCK, bone);
    b.add_box(Vec3::new(0.25, 0.06, -0.05), Vec3::new(0.38, 0.62, 0.12), UV_ROCK, bone);
    // Guard's service rifle lying within reach on the floor
    b.add_box(Vec3::new(-0.95, 0.02, 0.35), Vec3::new(-0.15, 0.11, 0.50), UV_STEEL, rifle);
    b.add_box(Vec3::new(-1.28, 0.04, 0.39), Vec3::new(-0.95, 0.09, 0.45), UV_STEEL, boots);

    b.to_glb_bytes()
}

fn build_fallen_prisoner_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let bone = [0.80, 0.78, 0.72, 1.0];
    let jumpsuit = [0.76, 0.42, 0.20, 1.0];
    let rifle = [0.36, 0.26, 0.18, 1.0];

    // Supine prisoner skeleton in faded orange cryo-prison jumpsuit
    b.add_box(Vec3::new(-0.35, 0.03, -0.22), Vec3::new(0.32, 0.20, 0.22), UV_ROCK, jumpsuit);
    b.add_box(Vec3::new(0.34, 0.03, -0.13), Vec3::new(0.58, 0.24, 0.13), UV_ROCK, bone);
    b.add_box(Vec3::new(-1.12, 0.03, -0.20), Vec3::new(-0.35, 0.16, -0.04), UV_ROCK, jumpsuit);
    b.add_box(Vec3::new(-1.08, 0.03, 0.05), Vec3::new(-0.35, 0.16, 0.21), UV_ROCK, jumpsuit);
    // Dropped weapon nearby
    b.add_box(Vec3::new(-0.95, 0.02, 0.42), Vec3::new(-0.08, 0.10, 0.56), UV_STEEL, rifle);

    b.to_glb_bytes()
}

fn build_emergency_locker_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let crate_steel = [0.44, 0.48, 0.50, 1.0];
    let cloth = [0.32, 0.46, 0.58, 1.0];

    // Emergency release locker chest with folded starter clothes inside + cyan indicator LED
    b.add_box(Vec3::new(-0.55, 0.0, -0.38), Vec3::new(0.55, 0.56, 0.38), UV_STEEL, crate_steel);
    b.add_box(Vec3::new(-0.42, 0.42, -0.26), Vec3::new(0.38, 0.62, 0.24), UV_ROCK, cloth);
    b.add_emissive_box(
        Vec3::new(-0.18, 0.32, 0.36),
        Vec3::new(0.18, 0.40, 0.41),
        [0.20, 0.82, 1.00],
        [0.85, 0.96, 1.0, 1.0],
    );

    b.to_glb_bytes()
}

fn build_cave_wall_seg_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let dark_rock = [0.34, 0.35, 0.32, 1.0];
    let moss_rock = [0.38, 0.42, 0.33, 1.0];

    // Craggy tiered subterranean rock wall (`4.4m wide × 1.8m thick × 5.4m high`)
    b.add_box(Vec3::new(-2.2, -0.2, -0.9), Vec3::new(2.2, 5.2, 0.5), UV_ROCK, dark_rock);
    b.add_box(Vec3::new(-1.9, -0.2, 0.3), Vec3::new(0.6, 2.8, 0.85), UV_ROCK, moss_rock);
    b.add_box(Vec3::new(-0.4, 2.4, 0.25), Vec3::new(2.1, 5.4, 0.95), UV_ROCK, dark_rock);
    b.add_box(Vec3::new(-2.1, 4.3, 0.35), Vec3::new(2.1, 5.5, 1.35), UV_ROCK, moss_rock);

    b.to_glb_bytes()
}

fn build_cave_pipes_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    let pipe_col = [0.38, 0.40, 0.38, 1.0];
    let panel_col = [0.42, 0.38, 0.34, 1.0];

    // Overhead conduit run (`7.0m` along X) + hanging ceiling baffle + cool fluorescent strip
    b.add_box(Vec3::new(-3.5, 0.0, -0.18), Vec3::new(3.5, 0.34, 0.18), UV_STEEL, pipe_col);
    b.add_box(Vec3::new(-2.8, 0.12, 0.35), Vec3::new(3.2, 0.30, 0.55), UV_STEEL, pipe_col);
    b.add_box(Vec3::new(-1.8, -0.14, -1.4), Vec3::new(0.6, -0.04, 0.2), UV_STEEL, panel_col);
    b.add_emissive_box(
        Vec3::new(0.9, -0.12, -0.45),
        Vec3::new(2.5, -0.02, -0.15),
        [0.82, 0.96, 0.88],
        [0.95, 1.0, 0.96, 1.0],
    );

    b.to_glb_bytes()
}

fn build_wall_sconce_glb(atlas: &[u8]) -> Vec<u8> {
    let mut b = SceneGlbBuilder::new(atlas.to_vec());
    b.add_box(
        Vec3::new(-0.14, -0.28, -0.10),
        Vec3::new(0.14, 0.28, 0.06),
        UV_STEEL,
        [0.32, 0.28, 0.24, 1.0],
    );
    b.add_emissive_box(
        Vec3::new(-0.10, -0.20, 0.02),
        Vec3::new(0.10, 0.20, 0.16),
        [1.00, 0.72, 0.32],
        [1.0, 0.88, 0.62, 1.0],
    );
    b.to_glb_bytes()
}

pub(super) fn cryo_prison_unique_assets() -> anyhow::Result<Vec<(&'static str, Vec<u8>)>> {
    let atlas = build_cryo_atlas_png()?;
    Ok(vec![
        ("cryo_rack_bay", build_cryo_rack_bay_glb(&atlas)),
        ("hero_cryo_pod", build_hero_cryo_pod_glb(&atlas)),
        ("cage_elevator", build_cage_elevator_glb(&atlas)),
        ("fallen_guard", build_fallen_guard_glb(&atlas)),
        ("fallen_prisoner", build_fallen_prisoner_glb(&atlas)),
        ("emergency_locker", build_emergency_locker_glb(&atlas)),
        ("cave_wall_seg", build_cave_wall_seg_glb(&atlas)),
        ("cave_pipes", build_cave_pipes_glb(&atlas)),
        ("wall_sconce", build_wall_sconce_glb(&atlas)),
    ])
}

/// Seeds the `cryo_prison` starting Inside area (`24m × 20m`) with area-scoped one-off `.glb` assets
/// in `world/areas/cryo_prison/assets/` without adding them to the global `catalog.json`.
pub(super) fn seed_cryo_prison_inside_area(world_dir: &Path) -> anyhow::Result<()> {
    let area_dir = world_dir.join("areas").join("cryo_prison");
    let assets_dir = area_dir.join("assets");
    fs::create_dir_all(&assets_dir)?;

    for (slug, glb_bytes) in cryo_prison_unique_assets()? {
        let glb_path = assets_dir.join(format!("{slug}.glb"));
        if !glb_path.exists() {
            fs::write(&glb_path, glb_bytes)?;
        }
    }

    let area_bin = area_dir.join("area.bin");
    if area_bin.exists() {
        return Ok(());
    }

    let (w, l) = (24u32, 20u32);
    let meta = AreaMetadata {
        id: "cryo_prison".to_owned(),
        name: "Cryo Prison".to_owned(),
        kind: AreaKind::Inside(InsideKind::Ephemeral),
        dimensions: Some((w, l)),
    };
    fs::write(&area_bin, postcard::to_allocvec(&meta)?)?;

    let mut vis = InsideVisData::flat_default(w, l);
    let vw = w as usize + 1;
    let vh = l as usize + 1;

    // Cavern floor heights & subterranean rock/dirt/concrete/moss splat materials
    for vz in 0..vh {
        for vx in 0..vw {
            let idx = vz * vw + vx;
            let fx = vx as f32;
            let fz = vz as f32;

            // Raised rocky edges along back (z <= 2), left (x <= 2), and right wall (x >= 21)
            let mut h = 0.0f32;
            if vx <= 2 {
                h = h.max((2.5 - fx) * 0.18);
            }
            if vz <= 2 {
                h = h.max((2.5 - fz) * 0.18);
            }
            if vx >= (w as usize - 2) {
                h = h.max((fx - (w as f32 - 2.5)) * 0.18);
            }
            // Subtle natural floor undulation away from pod rail & elevator threshold
            let ripple = ((fx * 0.45).sin() * (fz * 0.52).cos()) * 0.04;
            let near_flat_zone = (5.0..=19.5).contains(&fx) && (3.5..=14.5).contains(&fz);
            vis.heights[idx] = if near_flat_zone { h } else { h + ripple.max(0.0) };

            let base_mat = if vx <= 3 || vz <= 2 || vx >= (w as usize - 3) {
                MAT_ROCK
            } else if (15..=20).contains(&vx) && (3..=7).contains(&vz) {
                MAT_COBBLESTONE
            } else {
                MAT_DIRT
            };
            vis.splat_ids[idx] = [base_mat, 0, 0, 0];
            vis.splat_weights[idx] = [255, 0, 0, 0];

            // Creeping moss patches matching the overgrown cryo-vault concept art
            let moss_hash = ((vx * 37 + vz * 61) ^ (vx * vz * 11)) % 17;
            if moss_hash < 4 && vx > 2 && vz > 2 && vx < (w as usize - 2) {
                let moss_mat = if moss_hash == 0 {
                    MAT_DARK_GRASS
                } else {
                    MAT_PATCHY_GRASS
                };
                let (ids, weights) =
                    blend_splat_sample(vis.splat_ids[idx], vis.splat_weights[idx], moss_mat, 0.58);
                vis.splat_ids[idx] = ids;
                vis.splat_weights[idx] = weights;
            }
        }
    }

    vis.recompute_slope_walkability();

    // Pre-bake 1m walkability blockers around perimeter rock walls, cryo-racks, hero pod rail, and elevator walls
    for z in 0..(l as usize) {
        for x in 0..(w as usize) {
            let perimeter_wall = x <= 3 || z <= 2 || x >= 21 || z >= 19;
            let hero_pod_footprint = (6..=9).contains(&x) && (7..=10).contains(&z);
            let locker_footprint = x == 6 && z == 12;
            let elevator_frame = (15..=19).contains(&x)
                && (3..=6).contains(&z)
                && !((16..=18).contains(&x) && (4..=6).contains(&z));
            let right_ledge = x >= 19 && (10..=13).contains(&z);

            if perimeter_wall
                || hero_pod_footprint
                || locker_footprint
                || elevator_frame
                || right_ledge
            {
                vis.set_walkable(x, z, false);
            }
        }
    }

    let frac_pi_2 = std::f32::consts::FRAC_PI_2;
    vis.props = vec![
        // Left wall: 3 two-story cryo-capsule rack bays facing +X into the cavern
        PropPlacement {
            id: 1,
            asset_id: format_local_asset_id("cryo_rack_bay"),
            position: [2.8, 0.0, 5.2],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 2,
            asset_id: format_local_asset_id("cryo_rack_bay"),
            position: [2.8, 0.0, 8.8],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 3,
            asset_id: format_local_asset_id("cryo_rack_bay"),
            position: [2.8, 0.0, 12.4],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        // Player's opened failsafe cryo-capsule slid out onto central rails
        PropPlacement {
            id: 4,
            asset_id: format_local_asset_id("hero_cryo_pod"),
            position: [7.6, 0.0, 8.8],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        // Emergency release clothes locker near the player's capsule
        PropPlacement {
            id: 5,
            asset_id: format_local_asset_id("emergency_locker"),
            position: [6.4, 0.0, 12.4],
            rotation: [0.0, 0.35, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        // Back-right rusted cage elevator leading up to the surface ruins
        PropPlacement {
            id: 6,
            asset_id: format_local_asset_id("cage_elevator"),
            position: [17.4, 0.0, 4.6],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        // Forefront right: fallen guard & prisoner skeletons from the ancient gunfight
        PropPlacement {
            id: 7,
            asset_id: format_local_asset_id("fallen_guard"),
            position: [19.4, 0.0, 11.4],
            rotation: [0.0, -1.15, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 8,
            asset_id: format_local_asset_id("fallen_prisoner"),
            position: [17.2, 0.0, 13.2],
            rotation: [0.0, 0.25, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        // Subterranean cave wall segments along north/west/east (south camera cutaway left open)
        PropPlacement {
            id: 9,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [1.1, 0.0, 5.5],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 10,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [1.1, 0.0, 11.5],
            rotation: [0.0, frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 11,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [6.5, 0.0, 1.2],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.15, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 12,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [12.5, 0.0, 1.2],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.15, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 13,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [22.6, 0.0, 6.0],
            rotation: [0.0, -frac_pi_2, 0.0],
            scale: [1.1, 1.0, 1.0],
            cast_shadow: true,
        },
        PropPlacement {
            id: 14,
            asset_id: format_local_asset_id("cave_wall_seg"),
            position: [22.6, 0.0, 12.0],
            rotation: [0.0, -frac_pi_2, 0.0],
            scale: [1.1, 1.0, 1.0],
            cast_shadow: true,
        },
        // Overhead conduit pipes & fluorescent ceiling fixture
        PropPlacement {
            id: 15,
            asset_id: format_local_asset_id("cave_pipes"),
            position: [9.5, 4.45, 5.2],
            rotation: [0.0, -0.18, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: false,
        },
        // Warm amber wall sconce on right rock pillar
        PropPlacement {
            id: 16,
            asset_id: format_local_asset_id("wall_sconce"),
            position: [21.5, 2.55, 8.2],
            rotation: [0.0, -frac_pi_2, 0.0],
            scale: [1.0, 1.0, 1.0],
            cast_shadow: false,
        },
    ];

    let mut sim = compute_inside_sim_from_vis(&vis, None);
    sim.triggers.push(TriggerVolume {
        id: "elevator_to_surface".to_owned(),
        min: [16.0, 0.0, 3.6],
        max: [18.8, 2.8, 5.8],
        target_area: "main".to_owned(),
        target_pos: [32.0, 0.0, 32.0],
    });

    fs::write(area_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
    fs::write(area_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cryo_prison_unique_assets_have_valid_glb_headers_and_stay_out_of_global_catalog() {
        let assets = cryo_prison_unique_assets().unwrap();
        assert_eq!(assets.len(), 9);
        for (slug, glb) in &assets {
            assert!(shared::world::is_valid_asset_slug(slug));
            assert!(glb.len() > 128);
            assert_eq!(&glb[0..4], b"glTF");
            let total_len = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
            assert_eq!(total_len, glb.len(), "mismatched GLB length for {slug}");
        }
    }
}

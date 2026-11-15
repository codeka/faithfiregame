use std::f32::consts::TAU;
use std::fs;
use std::path::Path;

use glam::Vec3;
use serde_json::json;
use shared::world::{
    AreaKind, AreaMetadata, AssetEntry, InsideKind, InsideVisData, PropPlacement, TILE_SIZE,
    TILE_VERTICES, TileVisData, compute_inside_sim_from_vis, compute_sim_from_vis, pack_splat,
};

struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u16>,
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn push_tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
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

    fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: [f32; 4]) {
        self.push_tri(a, b, c, color);
        self.push_tri(a, c, d, color);
    }

    fn add_frustum(
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

    fn add_ellipsoid(&mut self, center: Vec3, radii: Vec3, rings: usize, sectors: usize, color: [f32; 4]) {
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

    fn to_glb_bytes(&self) -> Vec<u8> {
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

fn build_pine_tree_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bark = [0.38, 0.24, 0.14, 1.0];
    let pine_dark = [0.11, 0.34, 0.16, 1.0];
    let pine_mid = [0.15, 0.42, 0.19, 1.0];
    let pine_light = [0.19, 0.48, 0.22, 1.0];

    mb.add_frustum(Vec3::ZERO, 2.2, 0.36, 0.22, 7, bark);
    mb.add_frustum(Vec3::new(0.0, 1.6, 0.0), 2.0, 1.65, 0.15, 8, pine_dark);
    mb.add_frustum(Vec3::new(0.0, 2.8, 0.0), 1.8, 1.30, 0.12, 8, pine_mid);
    mb.add_frustum(Vec3::new(0.0, 3.9, 0.0), 1.6, 0.90, 0.02, 8, pine_light);
    // Small asymmetric side branch so orientation is clearly visible
    mb.add_frustum(Vec3::new(0.0, 1.1, 0.32), 0.7, 0.24, 0.06, 6, pine_dark);
    mb.to_glb_bytes()
}

fn build_oak_tree_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bark = [0.42, 0.27, 0.16, 1.0];
    let leaf1 = [0.22, 0.50, 0.18, 1.0];
    let leaf2 = [0.28, 0.57, 0.21, 1.0];
    let leaf3 = [0.18, 0.44, 0.15, 1.0];

    mb.add_frustum(Vec3::ZERO, 2.5, 0.48, 0.30, 8, bark);
    mb.add_ellipsoid(Vec3::new(0.0, 3.3, 0.0), Vec3::new(1.75, 1.25, 1.65), 5, 8, leaf1);
    mb.add_ellipsoid(Vec3::new(0.85, 3.0, 0.65), Vec3::new(1.1, 0.95, 1.05), 4, 7, leaf2);
    mb.add_ellipsoid(Vec3::new(-0.75, 2.9, -0.45), Vec3::new(1.0, 0.85, 1.0), 4, 7, leaf3);
    mb.to_glb_bytes()
}

fn build_rock_boulder_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.50, 0.51, 0.54, 1.0];
    let stone2 = [0.42, 0.44, 0.47, 1.0];
    mb.add_ellipsoid(Vec3::new(0.0, 0.55, 0.0), Vec3::new(1.15, 0.78, 0.95), 4, 7, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.38, 0.48), Vec3::new(0.62, 0.48, 0.55), 3, 6, stone2);
    mb.to_glb_bytes()
}

fn build_rock_cluster_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone1 = [0.48, 0.49, 0.52, 1.0];
    let stone2 = [0.56, 0.55, 0.53, 1.0];
    let stone3 = [0.39, 0.40, 0.43, 1.0];
    mb.add_ellipsoid(Vec3::new(-0.35, 0.45, -0.2), Vec3::new(0.85, 0.65, 0.75), 4, 6, stone1);
    mb.add_ellipsoid(Vec3::new(0.55, 0.32, 0.35), Vec3::new(0.65, 0.45, 0.55), 4, 6, stone2);
    mb.add_ellipsoid(Vec3::new(0.15, 0.22, -0.65), Vec3::new(0.48, 0.34, 0.44), 3, 6, stone3);
    mb.to_glb_bytes()
}

fn build_bush_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let bush1 = [0.19, 0.48, 0.17, 1.0];
    let bush2 = [0.25, 0.55, 0.21, 1.0];
    mb.add_ellipsoid(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.95, 0.62, 0.85), 4, 7, bush1);
    mb.add_ellipsoid(Vec3::new(0.38, 0.36, 0.32), Vec3::new(0.62, 0.48, 0.58), 4, 6, bush2);
    mb.to_glb_bytes()
}

fn build_ruin_stone_glb() -> Vec<u8> {
    let mut mb = MeshBuilder::new();
    let stone = [0.62, 0.60, 0.56, 1.0];
    let trim = [0.50, 0.49, 0.46, 1.0];
    // Plinth + tall carved pillar + projecting front altar ledge for unmistakable facing
    mb.add_frustum(Vec3::ZERO, 0.35, 0.85, 0.78, 4, trim);
    mb.add_frustum(Vec3::new(0.0, 0.35, 0.0), 2.15, 0.52, 0.42, 4, stone);
    mb.add_frustum(Vec3::new(0.0, 0.35, 0.52), 0.65, 0.32, 0.26, 4, trim);
    mb.to_glb_bytes()
}

/// Ensures starter `.glb` decoration assets, `catalog.json`, and default `main` / `village_shop` areas exist.
pub fn ensure_world_seeded(world_dir: &Path) -> anyhow::Result<()> {
    let dec_dir = world_dir.join("assets").join("decorations");
    fs::create_dir_all(&dec_dir)?;

    let assets: Vec<(&str, &str, &str, Vec<u8>)> = vec![
        ("pine_tree", "Pine Tree", "Nature", build_pine_tree_glb()),
        ("oak_tree", "Oak Tree", "Nature", build_oak_tree_glb()),
        ("rock_boulder", "Boulder", "Rocks", build_rock_boulder_glb()),
        ("rock_cluster", "Rock Cluster", "Rocks", build_rock_cluster_glb()),
        ("bush", "Bush", "Nature", build_bush_glb()),
        ("ruin_stone", "Ruin Stone", "Props", build_ruin_stone_glb()),
    ];

    let mut catalog = Vec::with_capacity(assets.len());
    for (id, name, category, glb_bytes) in assets {
        let filename = format!("{id}.glb");
        let glb_path = dec_dir.join(&filename);
        if !glb_path.exists() {
            fs::write(&glb_path, glb_bytes)?;
        }
        catalog.push(AssetEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            path: format!("/world/assets/decorations/{filename}"),
            category: category.to_owned(),
        });
    }

    let catalog_path = world_dir.join("assets").join("catalog.json");
    if !catalog_path.exists() {
        fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;
    }

    seed_main_outside_area(world_dir)?;
    seed_village_shop_inside_area(world_dir)?;
    Ok(())
}

fn seed_main_outside_area(world_dir: &Path) -> anyhow::Result<()> {
    let main_dir = world_dir.join("areas").join("main");
    let area_bin = main_dir.join("area.bin");
    if area_bin.exists() {
        return Ok(());
    }

    fs::create_dir_all(&main_dir)?;
    let meta = AreaMetadata {
        id: "main".to_owned(),
        name: "Main World".to_owned(),
        kind: AreaKind::Outside,
        dimensions: None,
    };
    fs::write(&area_bin, postcard::to_allocvec(&meta)?)?;

    // Seed the initial 3x3 Outside window (-1..=1, -1..=1) centered on (0, 0).
    for ty in -1..=1 {
        for tx in -1..=1 {
            let mut vis = TileVisData::flat_default();

            // Gentle welcoming features on starter tile (0, 0) while keeping center spawn (32, 32) flat.
            if tx == 0 && ty == 0 {
                for vz in 0..TILE_VERTICES {
                    for vx in 0..TILE_VERTICES {
                        let dx = vx as f32 - 14.0;
                        let dz = vz as f32 - 14.0;
                        let dist2 = dx * dx + dz * dz;
                        if dist2 < 100.0 {
                            let bump = (1.0 - dist2 / 100.0).powi(2) * 2.4;
                            vis.heights[vz * TILE_VERTICES + vx] = bump;
                        }
                    }
                }

                // A subtle dirt path leading south-east from spawn (32, 32)
                for z in 24..44 {
                    for x in 29..36 {
                        let d = ((x as f32 - 32.0).abs() / 3.0).min(1.0);
                        let dirt = ((1.0 - d) * 210.0) as u8;
                        let grass = 255u8.saturating_sub(dirt);
                        vis.splat_map[z * TILE_SIZE + x] = pack_splat(grass, dirt, 0, 0);
                    }
                }

                vis.props = vec![
                    PropPlacement {
                        id: 1,
                        asset_id: "ruin_stone".to_owned(),
                        position: [32.0, 0.0, 22.0],
                        rotation_y: 0.0,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 2,
                        asset_id: "pine_tree".to_owned(),
                        position: [20.0, 0.4, 18.0],
                        rotation_y: 0.7,
                        scale: [1.1, 1.1, 1.1],
                    },
                    PropPlacement {
                        id: 3,
                        asset_id: "oak_tree".to_owned(),
                        position: [44.0, 0.0, 26.0],
                        rotation_y: 2.1,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 4,
                        asset_id: "rock_cluster".to_owned(),
                        position: [24.0, 0.0, 42.0],
                        rotation_y: 1.2,
                        scale: [1.0, 1.0, 1.0],
                    },
                    PropPlacement {
                        id: 5,
                        asset_id: "bush".to_owned(),
                        position: [38.0, 0.0, 38.0],
                        rotation_y: 0.4,
                        scale: [1.0, 1.0, 1.0],
                    },
                ];
            }

            vis.recompute_slope_walkability();
            let sim = compute_sim_from_vis(&vis, None);

            let tile_dir = main_dir.join(format!("{tx},{ty}"));
            fs::create_dir_all(&tile_dir)?;
            fs::write(tile_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
            fs::write(tile_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
        }
    }

    Ok(())
}

fn seed_village_shop_inside_area(world_dir: &Path) -> anyhow::Result<()> {
    let shop_dir = world_dir.join("areas").join("village_shop");
    let area_bin = shop_dir.join("area.bin");
    if area_bin.exists() {
        return Ok(());
    }

    fs::create_dir_all(&shop_dir)?;
    let (w, l) = (24u32, 24u32);
    let meta = AreaMetadata {
        id: "village_shop".to_owned(),
        name: "Village Shop".to_owned(),
        kind: AreaKind::Inside(InsideKind::Ephemeral),
        dimensions: Some((w, l)),
    };
    fs::write(&area_bin, postcard::to_allocvec(&meta)?)?;

    let mut vis = InsideVisData::flat_default(w, l);
    for z in 0..l as usize {
        for x in 0..w as usize {
            let border = x < 2 || z < 2 || x >= (w as usize - 2) || z >= (l as usize - 2);
            vis.splat_map[z * w as usize + x] = if border {
                pack_splat(0, 0, 255, 0) // Rock floor border
            } else {
                pack_splat(0, 255, 0, 0) // Dirt/plank interior floor
            };
        }
    }
    vis.props.push(PropPlacement {
        id: 101,
        asset_id: "ruin_stone".to_owned(),
        position: [12.0, 0.0, 6.0],
        rotation_y: 0.0,
        scale: [1.0, 1.0, 1.0],
    });
    vis.recompute_slope_walkability();
    let sim = compute_inside_sim_from_vis(&vis, None);

    fs::write(shop_dir.join("vis.bin"), postcard::to_allocvec(&vis)?)?;
    fs::write(shop_dir.join("sim.bin"), postcard::to_allocvec(&sim)?)?;
    Ok(())
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

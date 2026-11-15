use serde::{Deserialize, Serialize};

use super::splat::{
    area_seed_from_id, convert_legacy_rgba_splat_grid, default_splat_ids, default_splat_weights,
    grass_noise_splat_at,
};
use super::types::{EntitySpawn, PropPlacement, TriggerVolume};
use super::walk_mask::{MAX_WALKABLE_SLOPE_DELTA, TILE_SIZE, TILE_VERTICES, WalkMask};

/// Server-authoritative simulation payload for one 64m × 64m Outside tile (`sim.bin`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileSimData {
    pub walkable: WalkMask,
    /// `64 × 64` cell center elevation samples (`z * 64 + x`).
    pub elevation: Vec<f32>,
    pub triggers: Vec<TriggerVolume>,
    pub spawns: Vec<EntitySpawn>,
}

impl TileSimData {
    pub fn flat_default() -> Self {
        Self {
            walkable: WalkMask::all_walkable(),
            elevation: vec![0.0; TILE_SIZE * TILE_SIZE],
            triggers: Vec::new(),
            spawns: Vec::new(),
        }
    }
}

/// Client visual payload for one 64m × 64m Outside tile (`vis.bin`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TileVisData {
    pub walkable: WalkMask,
    /// `65 × 65` heightmap vertices (`vz * 65 + vx`) so adjacent tiles share border vertices.
    pub heights: Vec<f32>,
    /// `65 × 65` top-4 material IDs per terrain vertex (`vz * 65 + vx`).
    pub splat_ids: Vec<[u8; 4]>,
    /// `65 × 65` top-4 normalized `u8` blend weights per terrain vertex (`vz * 65 + vx`, sum = `255`).
    pub splat_weights: Vec<[u8; 4]>,
    pub props: Vec<PropPlacement>,
}

impl TileVisData {
    pub fn flat_default() -> Self {
        let verts = TILE_VERTICES * TILE_VERTICES;
        Self {
            walkable: WalkMask::all_walkable(),
            heights: vec![0.0; verts],
            splat_ids: vec![default_splat_ids(); verts],
            splat_weights: vec![default_splat_weights(); verts],
            props: Vec::new(),
        }
    }

    /// Generates a flat tile populated with seamless world-space 3-tone grass noise
    /// (`MAT_GRASS`, `MAT_LIGHT_GRASS`, `MAT_DARK_GRASS`).
    pub fn seeded_grass(area_id: &str, tx: i32, ty: i32) -> Self {
        let verts = TILE_VERTICES * TILE_VERTICES;
        let seed = area_seed_from_id(area_id);
        let mut splat_ids = Vec::with_capacity(verts);
        let mut splat_weights = Vec::with_capacity(verts);
        for vz in 0..TILE_VERTICES {
            let wz = ty * (TILE_SIZE as i32) + vz as i32;
            for vx in 0..TILE_VERTICES {
                let wx = tx * (TILE_SIZE as i32) + vx as i32;
                let (ids, weights) = grass_noise_splat_at(seed, wx, wz);
                splat_ids.push(ids);
                splat_weights.push(weights);
            }
        }
        Self {
            walkable: WalkMask::all_walkable(),
            heights: vec![0.0; verts],
            splat_ids,
            splat_weights,
            props: Vec::new(),
        }
    }

    pub fn has_valid_dimensions(&self) -> bool {
        let verts = TILE_VERTICES * TILE_VERTICES;
        self.heights.len() == verts
            && self.splat_ids.len() == verts
            && self.splat_weights.len() == verts
    }

    /// Recomputes `walkable` cells based on terrain slope across each 1m cell.
    pub fn recompute_slope_walkability(&mut self) {
        if self.heights.len() != TILE_VERTICES * TILE_VERTICES {
            return;
        }
        for z in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let h00 = self.heights[z * TILE_VERTICES + x];
                let h10 = self.heights[z * TILE_VERTICES + x + 1];
                let h01 = self.heights[(z + 1) * TILE_VERTICES + x];
                let h11 = self.heights[(z + 1) * TILE_VERTICES + x + 1];
                let min_h = h00.min(h10).min(h01).min(h11);
                let max_h = h00.max(h10).max(h01).max(h11);
                self.walkable
                    .set(x, z, (max_h - min_h) <= MAX_WALKABLE_SLOPE_DELTA);
            }
        }
    }

    /// Decodes a postcard-encoded tile visual payload, falling back across legacy `v3`
    /// (`splat_map: Vec<u32>`), `v2` (`rotation: [f32; 3]`), and `v1` (`rotation_y`) formats
    /// so existing world files migrate seamlessly.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        #[derive(Deserialize)]
        struct LegacySplatTile<P> {
            walkable: WalkMask,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<P>,
        }

        fn upgrade_legacy<P: Into<PropPlacement>>(old: LegacySplatTile<P>) -> TileVisData {
            let (splat_ids, splat_weights) =
                convert_legacy_rgba_splat_grid(&old.splat_map, TILE_SIZE, TILE_SIZE);
            TileVisData {
                walkable: old.walkable,
                heights: old.heights,
                splat_ids,
                splat_weights,
                props: old.props.into_iter().map(Into::into).collect(),
            }
        }

        match super::types::decode_exact::<Self>(bytes) {
            Ok(vis) if vis.has_valid_dimensions() => Ok(vis),
            Ok(_) => Err(postcard::Error::SerdeDeCustom),
            Err(primary_err) => super::types::decode_exact::<LegacySplatTile<PropPlacement>>(bytes)
                .map(upgrade_legacy)
                .or_else(|_| {
                    super::types::decode_exact::<LegacySplatTile<super::types::PropPlacementV2>>(
                        bytes,
                    )
                    .map(upgrade_legacy)
                })
                .or_else(|_| {
                    super::types::decode_exact::<LegacySplatTile<super::types::LegacyPropPlacement>>(
                        bytes,
                    )
                    .map(upgrade_legacy)
                })
                .map_err(|_| primary_err),
        }
    }
}

/// Derives `TileSimData` from a `TileVisData`, computing cell elevations and steep-slope walkability
/// while preserving existing triggers and spawns.
pub fn compute_sim_from_vis(vis: &TileVisData, existing: Option<&TileSimData>) -> TileSimData {
    let mut elevation = vec![0.0; TILE_SIZE * TILE_SIZE];
    let mut walkable = vis.walkable;

    if vis.heights.len() == TILE_VERTICES * TILE_VERTICES {
        for z in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let h00 = vis.heights[z * TILE_VERTICES + x];
                let h10 = vis.heights[z * TILE_VERTICES + x + 1];
                let h01 = vis.heights[(z + 1) * TILE_VERTICES + x];
                let h11 = vis.heights[(z + 1) * TILE_VERTICES + x + 1];
                elevation[z * TILE_SIZE + x] = 0.25 * (h00 + h10 + h01 + h11);

                let min_h = h00.min(h10).min(h01).min(h11);
                let max_h = h00.max(h10).max(h01).max(h11);
                if (max_h - min_h) > MAX_WALKABLE_SLOPE_DELTA {
                    walkable.set(x, z, false);
                }
            }
        }
    }

    TileSimData {
        walkable,
        elevation,
        triggers: existing.map(|s| s.triggers.clone()).unwrap_or_default(),
        spawns: existing.map(|s| s.spawns.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{MAT_BRICK, MAT_COBBLESTONE, MAT_DIRT, MAT_GRASS, pack_splat};

    #[test]
    fn tile_vis_and_sim_roundtrip() {
        let mut vis = TileVisData::flat_default();
        vis.heights[10 * TILE_VERTICES + 10] = 5.0;
        vis.splat_ids[12 * TILE_VERTICES + 12] = [MAT_COBBLESTONE, MAT_BRICK, MAT_DIRT, 0];
        vis.splat_weights[12 * TILE_VERTICES + 12] = [140, 80, 35, 0];
        vis.recompute_slope_walkability();
        assert!(!vis.walkable.get(10, 10));
        assert!(vis.walkable.get(0, 0));

        let bytes = postcard::to_allocvec(&vis).unwrap();
        let decoded = TileVisData::from_postcard_bytes(&bytes).unwrap();
        assert_eq!(vis, decoded);
        assert!(decoded.has_valid_dimensions());

        let sim = compute_sim_from_vis(&vis, None);
        assert!(!sim.walkable.get(10, 10));
        assert!(sim.elevation[10 * TILE_SIZE + 10] > 1.0);
    }

    #[test]
    fn legacy_tile_vis_decoding_migrates_splat_and_props() {
        #[derive(Serialize)]
        struct LegacyProp {
            id: u64,
            asset_id: String,
            position: [f32; 3],
            rotation_y: f32,
            scale: [f32; 3],
        }
        #[derive(Serialize)]
        struct LegacyTile {
            walkable: WalkMask,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<LegacyProp>,
        }

        let base = TileVisData::flat_default();
        let mut legacy_splat = vec![pack_splat(255, 0, 0, 0); TILE_SIZE * TILE_SIZE];
        legacy_splat[0] = pack_splat(0, 255, 0, 0);
        let legacy = LegacyTile {
            walkable: base.walkable,
            heights: base.heights,
            splat_map: legacy_splat,
            props: vec![LegacyProp {
                id: 7,
                asset_id: "ruin_stone".to_owned(),
                position: [12.0, 1.5, 34.0],
                rotation_y: 1.25,
                scale: [1.0, 1.0, 1.0],
            }],
        };

        let bytes = postcard::to_allocvec(&legacy).unwrap();
        let decoded = TileVisData::from_postcard_bytes(&bytes).unwrap();
        assert!(decoded.has_valid_dimensions());
        assert_eq!(decoded.splat_ids[0], [MAT_DIRT, 0, 0, 0]);
        assert_eq!(decoded.splat_weights[0], [255, 0, 0, 0]);
        assert_eq!(decoded.splat_ids[10 * TILE_VERTICES + 10], [MAT_GRASS, 0, 0, 0]);
        assert_eq!(decoded.props.len(), 1);
        assert_eq!(decoded.props[0].rotation, [0.0, 1.25, 0.0]);
        assert!(decoded.props[0].cast_shadow);
    }

    #[test]
    fn v2_and_v3_tile_vis_decoding_migrate_splat_and_props() {
        #[derive(Serialize)]
        struct V2Prop {
            id: u64,
            asset_id: String,
            position: [f32; 3],
            rotation: [f32; 3],
            scale: [f32; 3],
        }
        #[derive(Serialize)]
        struct V2Tile {
            walkable: WalkMask,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<V2Prop>,
        }
        #[derive(Serialize)]
        struct V3Tile {
            walkable: WalkMask,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<PropPlacement>,
        }

        let base = TileVisData::flat_default();
        let legacy_splat = vec![pack_splat(255, 0, 0, 0); TILE_SIZE * TILE_SIZE];
        let v2 = V2Tile {
            walkable: base.walkable,
            heights: base.heights.clone(),
            splat_map: legacy_splat.clone(),
            props: vec![V2Prop {
                id: 10,
                asset_id: "boulder".to_owned(),
                position: [15.0, 2.0, 20.0],
                rotation: [0.3, 0.8, 0.1],
                scale: [1.0, 1.0, 1.0],
            }],
        };

        let v2_bytes = postcard::to_allocvec(&v2).unwrap();
        let decoded_v2 = TileVisData::from_postcard_bytes(&v2_bytes).unwrap();
        assert!(decoded_v2.has_valid_dimensions());
        assert_eq!(decoded_v2.props.len(), 1);
        assert_eq!(decoded_v2.props[0].rotation, [0.3, 0.8, 0.1]);
        assert!(decoded_v2.props[0].cast_shadow);

        let v3 = V3Tile {
            walkable: base.walkable,
            heights: base.heights,
            splat_map: legacy_splat,
            props: vec![PropPlacement {
                id: 12,
                asset_id: "boulder".to_owned(),
                position: [18.0, 3.0, 19.0],
                rotation: [0.0, 0.5, 0.0],
                scale: [1.0, 1.0, 1.0],
                cast_shadow: false,
            }],
        };
        let v3_bytes = postcard::to_allocvec(&v3).unwrap();
        let decoded_v3 = TileVisData::from_postcard_bytes(&v3_bytes).unwrap();
        assert!(decoded_v3.has_valid_dimensions());
        assert_eq!(decoded_v3.props.len(), 1);
        assert!(!decoded_v3.props[0].cast_shadow);
    }

    #[test]
    fn seeded_grass_tiles_blend_three_tones_and_match_across_borders() {
        use crate::world::{MAT_DARK_GRASS, MAT_LIGHT_GRASS};

        let t00 = TileVisData::seeded_grass("meadow_test", 0, 0);
        assert!(t00.has_valid_dimensions());

        let mut dominant_counts = [0usize; 6];
        for i in 0..t00.splat_ids.len() {
            let ids = t00.splat_ids[i];
            let weights = t00.splat_weights[i];
            let sum: u16 = weights.iter().map(|&w| w as u16).sum();
            assert_eq!(sum, 255);
            assert!(weights.windows(2).all(|w| w[0] >= w[1]));
            for slot in 0..4 {
                if weights[slot] > 0 {
                    assert!(
                        matches!(ids[slot], MAT_GRASS | MAT_LIGHT_GRASS | MAT_DARK_GRASS),
                        "unexpected grass noise mat id {}",
                        ids[slot]
                    );
                }
            }
            dominant_counts[ids[0] as usize] += 1;
        }
        assert!(dominant_counts[MAT_GRASS as usize] >= 400);
        assert!(dominant_counts[MAT_LIGHT_GRASS as usize] >= 400);
        assert!(dominant_counts[MAT_DARK_GRASS as usize] >= 400);

        let t10 = TileVisData::seeded_grass("meadow_test", 1, 0);
        let tm10 = TileVisData::seeded_grass("meadow_test", -1, 0);
        for vz in 0..TILE_VERTICES {
            let right_00 = vz * TILE_VERTICES + (TILE_VERTICES - 1);
            let left_10 = vz * TILE_VERTICES;
            assert_eq!(t00.splat_ids[right_00], t10.splat_ids[left_10]);
            assert_eq!(t00.splat_weights[right_00], t10.splat_weights[left_10]);

            let right_m10 = vz * TILE_VERTICES + (TILE_VERTICES - 1);
            let left_00 = vz * TILE_VERTICES;
            assert_eq!(tm10.splat_ids[right_m10], t00.splat_ids[left_00]);
            assert_eq!(tm10.splat_weights[right_m10], t00.splat_weights[left_00]);
        }

        let t01 = TileVisData::seeded_grass("meadow_test", 0, 1);
        let t0m1 = TileVisData::seeded_grass("meadow_test", 0, -1);
        for vx in 0..TILE_VERTICES {
            let bottom_00 = (TILE_VERTICES - 1) * TILE_VERTICES + vx;
            let top_01 = vx;
            assert_eq!(t00.splat_ids[bottom_00], t01.splat_ids[top_01]);
            assert_eq!(t00.splat_weights[bottom_00], t01.splat_weights[top_01]);

            let bottom_0m1 = (TILE_VERTICES - 1) * TILE_VERTICES + vx;
            let top_00 = vx;
            assert_eq!(t0m1.splat_ids[bottom_0m1], t00.splat_ids[top_00]);
            assert_eq!(t0m1.splat_weights[bottom_0m1], t00.splat_weights[top_00]);
        }
    }
}

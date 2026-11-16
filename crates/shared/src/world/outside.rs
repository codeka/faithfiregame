use serde::{Deserialize, Serialize};

use super::road::RoadSpline;
use super::splat::{
    area_seed_from_id, default_splat_ids, default_splat_weights, grass_noise_splat_at,
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
    pub roads: Vec<RoadSpline>,
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
            roads: Vec::new(),
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
            roads: Vec::new(),
        }
    }

    pub fn has_valid_dimensions(&self) -> bool {
        let verts = TILE_VERTICES * TILE_VERTICES;
        self.heights.len() == verts
            && self.splat_ids.len() == verts
            && self.splat_weights.len() == verts
    }

    /// Returns `true` when the terrain slope across cell `(x, z)` is within walkable limits.
    pub fn is_cell_slope_walkable(&self, x: usize, z: usize) -> bool {
        if x >= TILE_SIZE || z >= TILE_SIZE || self.heights.len() != TILE_VERTICES * TILE_VERTICES {
            return false;
        }
        let h00 = self.heights[z * TILE_VERTICES + x];
        let h10 = self.heights[z * TILE_VERTICES + x + 1];
        let h01 = self.heights[(z + 1) * TILE_VERTICES + x];
        let h11 = self.heights[(z + 1) * TILE_VERTICES + x + 1];
        let min_h = h00.min(h10).min(h01).min(h11);
        let max_h = h00.max(h10).max(h01).max(h11);
        (max_h - min_h) <= MAX_WALKABLE_SLOPE_DELTA
    }

    /// Recomputes `walkable` cells based purely on terrain slope across each 1m cell.
    pub fn recompute_slope_walkability(&mut self) {
        if self.heights.len() != TILE_VERTICES * TILE_VERTICES {
            return;
        }
        for z in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let ok = self.is_cell_slope_walkable(x, z);
                self.walkable.set(x, z, ok);
            }
        }
    }

    /// Recomputes slope walkability only for cells inside `[min_x..=max_x, min_z..=max_z]`.
    pub fn recompute_slope_walkability_in_rect(
        &mut self,
        min_x: usize,
        min_z: usize,
        max_x: usize,
        max_z: usize,
    ) {
        if self.heights.len() != TILE_VERTICES * TILE_VERTICES || TILE_SIZE == 0 {
            return;
        }
        let x1 = max_x.min(TILE_SIZE - 1);
        let z1 = max_z.min(TILE_SIZE - 1);
        if min_x > x1 || min_z > z1 {
            return;
        }
        for z in min_z..=z1 {
            for x in min_x..=x1 {
                let ok = self.is_cell_slope_walkable(x, z);
                self.walkable.set(x, z, ok);
            }
        }
    }

    /// Marks cells steeper than `MAX_WALKABLE_SLOPE_DELTA` impassable while preserving manual blockers on flat ground.
    pub fn enforce_slope_walkability(&mut self) {
        if self.heights.len() != TILE_VERTICES * TILE_VERTICES {
            return;
        }
        for z in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                if !self.is_cell_slope_walkable(x, z) {
                    self.walkable.set(x, z, false);
                }
            }
        }
    }

    /// Decodes and validates a postcard-encoded tile visual payload.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        let vis = super::types::decode_exact::<Self>(bytes)?;
        if !vis.has_valid_dimensions() {
            return Err(postcard::Error::SerdeDeCustom);
        }
        Ok(vis)
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
    use crate::world::{
        MAT_BRICK, MAT_COBBLESTONE, MAT_DIRT, MAT_GRASS, ROAD_MAT_ASPHALT, RoadSpline, RoadStyle,
    };

    #[test]
    fn tile_vis_and_sim_roundtrip() {
        let mut vis = TileVisData::flat_default();
        vis.heights[10 * TILE_VERTICES + 10] = 5.0;
        vis.splat_ids[12 * TILE_VERTICES + 12] = [MAT_COBBLESTONE, MAT_BRICK, MAT_DIRT, 0];
        vis.splat_weights[12 * TILE_VERTICES + 12] = [140, 80, 35, 0];
        vis.roads.push(RoadSpline {
            id: 42,
            style: RoadStyle::Straight,
            material: ROAD_MAT_ASPHALT,
            width: 4.5,
            shoulder: 2.5,
            points: vec![[4.0, 0.0, 4.0], [20.0, 1.5, 4.0]],
            profile_heights: vec![0.0, 0.75, 1.5],
        });
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

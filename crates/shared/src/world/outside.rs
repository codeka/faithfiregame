use serde::{Deserialize, Serialize};

use super::types::{EntitySpawn, PropPlacement, TriggerVolume, pack_splat};
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
    /// `64 × 64` RGBA8 packed splat weights (`z * 64 + x`).
    pub splat_map: Vec<u32>,
    pub props: Vec<PropPlacement>,
}

impl TileVisData {
    pub fn flat_default() -> Self {
        Self {
            walkable: WalkMask::all_walkable(),
            heights: vec![0.0; TILE_VERTICES * TILE_VERTICES],
            splat_map: vec![pack_splat(255, 0, 0, 0); TILE_SIZE * TILE_SIZE],
            props: Vec::new(),
        }
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

    /// Decodes a postcard-encoded tile visual payload, falling back to legacy single-axis
    /// `rotation_y` prop records so existing world files load cleanly.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        #[derive(Deserialize)]
        struct LegacyTileVisData {
            walkable: WalkMask,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<super::types::LegacyPropPlacement>,
        }

        match postcard::from_bytes::<Self>(bytes) {
            Ok(vis) => Ok(vis),
            Err(primary_err) => postcard::from_bytes::<LegacyTileVisData>(bytes)
                .map(|legacy| Self {
                    walkable: legacy.walkable,
                    heights: legacy.heights,
                    splat_map: legacy.splat_map,
                    props: legacy.props.into_iter().map(Into::into).collect(),
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

    #[test]
    fn tile_vis_and_sim_roundtrip() {
        let mut vis = TileVisData::flat_default();
        vis.heights[10 * TILE_VERTICES + 10] = 5.0;
        vis.recompute_slope_walkability();
        assert!(!vis.walkable.get(10, 10));
        assert!(vis.walkable.get(0, 0));

        let bytes = postcard::to_allocvec(&vis).unwrap();
        let decoded: TileVisData = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(vis, decoded);

        let sim = compute_sim_from_vis(&vis, None);
        assert!(!sim.walkable.get(10, 10));
        assert!(sim.elevation[10 * TILE_SIZE + 10] > 1.0);
    }

    #[test]
    fn legacy_tile_vis_decoding_preserves_rotation_y() {
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
        let legacy = LegacyTile {
            walkable: base.walkable,
            heights: base.heights,
            splat_map: base.splat_map,
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
        assert_eq!(decoded.props.len(), 1);
        assert_eq!(decoded.props[0].rotation, [0.0, 1.25, 0.0]);
    }
}

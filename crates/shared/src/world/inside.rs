use serde::{Deserialize, Serialize};

use super::splat::{
    area_seed_from_id, convert_legacy_rgba_splat_grid, default_splat_ids, default_splat_weights,
    grass_noise_splat_at,
};
use super::types::{EntitySpawn, PropPlacement, TriggerVolume};
use super::walk_mask::MAX_WALKABLE_SLOPE_DELTA;

/// Server-authoritative simulation payload for an arbitrary-size Inside area (`sim.bin`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsideSimData {
    pub width: u32,
    pub length: u32,
    /// Packed 1m walkability bitmask (`ceil((width * length) / 64)` words, `idx = z * width + x`).
    pub walkable: Vec<u64>,
    pub elevation: Vec<f32>,
    pub triggers: Vec<TriggerVolume>,
    pub spawns: Vec<EntitySpawn>,
}

impl InsideSimData {
    pub fn flat_default(width: u32, length: u32) -> Self {
        let cells = (width as usize) * (length as usize);
        Self {
            width,
            length,
            walkable: vec![u64::MAX; cells.div_ceil(64)],
            elevation: vec![0.0; cells],
            triggers: Vec::new(),
            spawns: Vec::new(),
        }
    }
}

/// Client visual payload for an arbitrary-size Inside area (`vis.bin`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsideVisData {
    pub width: u32,
    pub length: u32,
    pub walkable: Vec<u64>,
    /// Optional `(width + 1) × (length + 1)` heightmap vertices; empty when purely `.glb`-authored.
    pub heights: Vec<f32>,
    /// Optional `(width + 1) × (length + 1)` top-4 material IDs per vertex; empty when `.glb`-authored.
    pub splat_ids: Vec<[u8; 4]>,
    /// Optional `(width + 1) × (length + 1)` top-4 normalized `u8` weights (sum = `255`).
    pub splat_weights: Vec<[u8; 4]>,
    pub props: Vec<PropPlacement>,
}

impl InsideVisData {
    pub fn flat_default(width: u32, length: u32) -> Self {
        let cells = (width as usize) * (length as usize);
        let verts = (width as usize + 1) * (length as usize + 1);
        Self {
            width,
            length,
            walkable: vec![u64::MAX; cells.div_ceil(64)],
            heights: vec![0.0; verts],
            splat_ids: vec![default_splat_ids(); verts],
            splat_weights: vec![default_splat_weights(); verts],
            props: Vec::new(),
        }
    }

    /// Generates an Inside terrain populated with 3-tone grass noise
    /// (`MAT_GRASS`, `MAT_LIGHT_GRASS`, `MAT_DARK_GRASS`).
    pub fn seeded_grass(area_id: &str, width: u32, length: u32) -> Self {
        let cells = (width as usize) * (length as usize);
        let vw = width as usize + 1;
        let vh = length as usize + 1;
        let verts = vw * vh;
        let seed = area_seed_from_id(area_id);
        let mut splat_ids = Vec::with_capacity(verts);
        let mut splat_weights = Vec::with_capacity(verts);
        for vz in 0..vh {
            for vx in 0..vw {
                let (ids, weights) = grass_noise_splat_at(seed, vx as i32, vz as i32);
                splat_ids.push(ids);
                splat_weights.push(weights);
            }
        }
        Self {
            width,
            length,
            walkable: vec![u64::MAX; cells.div_ceil(64)],
            heights: vec![0.0; verts],
            splat_ids,
            splat_weights,
            props: Vec::new(),
        }
    }

    pub fn has_valid_dimensions(&self) -> bool {
        let verts = (self.width as usize + 1) * (self.length as usize + 1);
        let heights_ok = self.heights.is_empty() || self.heights.len() == verts;
        let splat_ok = (self.splat_ids.is_empty() && self.splat_weights.is_empty())
            || (self.splat_ids.len() == verts && self.splat_weights.len() == verts);
        heights_ok && splat_ok
    }

    pub fn get_walkable(&self, x: usize, z: usize) -> bool {
        let w = self.width as usize;
        let l = self.length as usize;
        if x >= w || z >= l {
            return false;
        }
        let idx = z * w + x;
        self.walkable
            .get(idx / 64)
            .map(|word| (word >> (idx % 64)) & 1 != 0)
            .unwrap_or(false)
    }

    pub fn set_walkable(&mut self, x: usize, z: usize, walkable: bool) {
        let w = self.width as usize;
        let l = self.length as usize;
        if x >= w || z >= l {
            return;
        }
        let idx = z * w + x;
        if let Some(word) = self.walkable.get_mut(idx / 64) {
            if walkable {
                *word |= 1u64 << (idx % 64);
            } else {
                *word &= !(1u64 << (idx % 64));
            }
        }
    }

    pub fn recompute_slope_walkability(&mut self) {
        let w = self.width as usize;
        let l = self.length as usize;
        let vw = w + 1;
        if self.heights.len() != vw * (l + 1) {
            return;
        }
        for z in 0..l {
            for x in 0..w {
                let h00 = self.heights[z * vw + x];
                let h10 = self.heights[z * vw + x + 1];
                let h01 = self.heights[(z + 1) * vw + x];
                let h11 = self.heights[(z + 1) * vw + x + 1];
                let min_h = h00.min(h10).min(h01).min(h11);
                let max_h = h00.max(h10).max(h01).max(h11);
                self.set_walkable(x, z, (max_h - min_h) <= MAX_WALKABLE_SLOPE_DELTA);
            }
        }
    }

    /// Decodes a postcard-encoded inside visual payload, falling back across prior `v3`
    /// (`splat_map: Vec<u32>`), `v2` (`rotation: [f32; 3]`), and legacy `v1` (`rotation_y`) formats
    /// so existing interior world files migrate seamlessly.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        #[derive(Deserialize)]
        struct LegacySplatInside<P> {
            width: u32,
            length: u32,
            walkable: Vec<u64>,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<P>,
        }

        fn upgrade_legacy<P: Into<PropPlacement>>(old: LegacySplatInside<P>) -> InsideVisData {
            let (splat_ids, splat_weights) = convert_legacy_rgba_splat_grid(
                &old.splat_map,
                old.width as usize,
                old.length as usize,
            );
            InsideVisData {
                width: old.width,
                length: old.length,
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
            Err(primary_err) => {
                super::types::decode_exact::<LegacySplatInside<PropPlacement>>(bytes)
                    .map(upgrade_legacy)
                    .or_else(|_| {
                        super::types::decode_exact::<LegacySplatInside<super::types::PropPlacementV2>>(
                            bytes,
                        )
                        .map(upgrade_legacy)
                    })
                    .or_else(|_| {
                        super::types::decode_exact::<
                            LegacySplatInside<super::types::LegacyPropPlacement>,
                        >(bytes)
                        .map(upgrade_legacy)
                    })
                    .map_err(|_| primary_err)
            }
        }
    }
}

/// Derives `InsideSimData` from an `InsideVisData`, preserving existing triggers and spawns.
pub fn compute_inside_sim_from_vis(
    vis: &InsideVisData,
    existing: Option<&InsideSimData>,
) -> InsideSimData {
    let w = vis.width as usize;
    let l = vis.length as usize;
    let vw = w + 1;
    let mut elevation = vec![0.0; w * l];
    let walkable = vis.walkable.clone();

    if vis.heights.len() == vw * (l + 1) {
        for z in 0..l {
            for x in 0..w {
                let h00 = vis.heights[z * vw + x];
                let h10 = vis.heights[z * vw + x + 1];
                let h01 = vis.heights[(z + 1) * vw + x];
                let h11 = vis.heights[(z + 1) * vw + x + 1];
                elevation[z * w + x] = 0.25 * (h00 + h10 + h01 + h11);
            }
        }
    }

    InsideSimData {
        width: vis.width,
        length: vis.length,
        walkable,
        elevation,
        triggers: existing.map(|s| s.triggers.clone()).unwrap_or_default(),
        spawns: existing.map(|s| s.spawns.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{MAT_COBBLESTONE, MAT_DIRT, pack_splat};

    #[test]
    fn inside_vis_roundtrip_and_legacy_splat_migration() {
        let mut vis = InsideVisData::flat_default(4, 4);
        vis.splat_ids[0] = [MAT_COBBLESTONE, 0, 0, 0];
        vis.splat_weights[0] = [255, 0, 0, 0];
        assert!(vis.has_valid_dimensions());

        let bytes = postcard::to_allocvec(&vis).unwrap();
        let decoded = InsideVisData::from_postcard_bytes(&bytes).unwrap();
        assert_eq!(vis, decoded);

        #[derive(Serialize)]
        struct LegacyV3Inside {
            width: u32,
            length: u32,
            walkable: Vec<u64>,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<PropPlacement>,
        }
        let legacy = LegacyV3Inside {
            width: 4,
            length: 4,
            walkable: vec![u64::MAX; 1],
            heights: vec![0.0; 25],
            splat_map: vec![pack_splat(0, 255, 0, 0); 16],
            props: Vec::new(),
        };
        let legacy_bytes = postcard::to_allocvec(&legacy).unwrap();
        let migrated = InsideVisData::from_postcard_bytes(&legacy_bytes).unwrap();
        assert!(migrated.has_valid_dimensions());
        assert_eq!(migrated.splat_ids[0], [MAT_DIRT, 0, 0, 0]);
        assert_eq!(migrated.splat_weights[0], [255, 0, 0, 0]);
    }
}

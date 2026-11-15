use serde::{Deserialize, Serialize};

use super::types::{EntitySpawn, PropPlacement, TriggerVolume, pack_splat};
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
    /// Optional `width × length` splat map; empty when purely `.glb`-authored.
    pub splat_map: Vec<u32>,
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
            splat_map: vec![pack_splat(255, 0, 0, 0); cells],
            props: Vec::new(),
        }
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

    /// Decodes a postcard-encoded inside visual payload, falling back to legacy single-axis
    /// `rotation_y` prop records so existing interior world files load cleanly.
    pub fn from_postcard_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        #[derive(Deserialize)]
        struct LegacyInsideVisData {
            width: u32,
            length: u32,
            walkable: Vec<u64>,
            heights: Vec<f32>,
            splat_map: Vec<u32>,
            props: Vec<super::types::LegacyPropPlacement>,
        }

        match postcard::from_bytes::<Self>(bytes) {
            Ok(vis) => Ok(vis),
            Err(primary_err) => postcard::from_bytes::<LegacyInsideVisData>(bytes)
                .map(|legacy| Self {
                    width: legacy.width,
                    length: legacy.length,
                    walkable: legacy.walkable,
                    heights: legacy.heights,
                    splat_map: legacy.splat_map,
                    props: legacy.props.into_iter().map(Into::into).collect(),
                })
                .map_err(|_| primary_err),
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

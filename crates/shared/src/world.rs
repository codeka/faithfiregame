use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Number of 1m × 1m navigation cells per side of an Outside terrain tile.
pub const TILE_SIZE: usize = 64;

/// Number of heightmap vertices per side of an Outside terrain tile (`TILE_SIZE + 1` to share border vertices).
pub const TILE_VERTICES: usize = TILE_SIZE + 1;

/// Maximum height delta across a 1m cell before it is considered too steep to walk.
pub const MAX_WALKABLE_SLOPE_DELTA: f32 = 1.2;

/// Compact 64 × 64 (512-byte) 1m navigation walkability bitmask for an Outside tile.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WalkMask(pub [u64; TILE_SIZE]);

impl WalkMask {
    pub const fn all_walkable() -> Self {
        Self([u64::MAX; TILE_SIZE])
    }

    pub const fn all_blocked() -> Self {
        Self([0; TILE_SIZE])
    }

    #[inline]
    pub fn get(&self, x: usize, z: usize) -> bool {
        if x >= TILE_SIZE || z >= TILE_SIZE {
            return false;
        }
        (self.0[z] >> x) & 1 != 0
    }

    #[inline]
    pub fn set(&mut self, x: usize, z: usize, walkable: bool) {
        if x >= TILE_SIZE || z >= TILE_SIZE {
            return;
        }
        if walkable {
            self.0[z] |= 1u64 << x;
        } else {
            self.0[z] &= !(1u64 << x);
        }
    }
}

impl Default for WalkMask {
    fn default() -> Self {
        Self::all_walkable()
    }
}

impl fmt::Debug for WalkMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let walkable_count: u32 = self.0.iter().map(|row| row.count_ones()).sum();
        write!(f, "WalkMask({walkable_count}/4096 walkable)")
    }
}

impl Serialize for WalkMask {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tup = serializer.serialize_tuple(TILE_SIZE)?;
        for word in &self.0 {
            tup.serialize_element(word)?;
        }
        tup.end()
    }
}

impl<'de> Deserialize<'de> for WalkMask {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WalkMaskVisitor;

        impl<'de> Visitor<'de> for WalkMaskVisitor {
            type Value = WalkMask;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a 64-element sequence of u64 walkability rows")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut rows = [0u64; TILE_SIZE];
                for (i, slot) in rows.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(WalkMask(rows))
            }
        }

        deserializer.deserialize_tuple(TILE_SIZE, WalkMaskVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsideKind {
    Generated,
    Ephemeral,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AreaKind {
    Outside,
    Inside(InsideKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaMetadata {
    pub id: String,
    pub name: String,
    pub kind: AreaKind,
    /// `(width, length)` in meters for fixed Inside areas; `None` for Outside or Generated areas.
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerVolume {
    pub id: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub target_area: String,
    pub target_pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySpawn {
    pub id: u64,
    pub template_id: String,
    pub position: [f32; 3],
    pub leash_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropPlacement {
    pub id: u64,
    pub asset_id: String,
    pub position: [f32; 3],
    pub rotation_y: f32,
    pub scale: [f32; 3],
}

/// Packs 4 RGBA8 texture-splat weights into a little-endian `u32` matching WebGL2 `RGBA8` byte layout.
#[inline]
pub const fn pack_splat(layer0: u8, layer1: u8, layer2: u8, layer3: u8) -> u32 {
    u32::from_le_bytes([layer0, layer1, layer2, layer3])
}

#[inline]
pub const fn unpack_splat(packed: u32) -> [u8; 4] {
    packed.to_le_bytes()
}

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
}

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

/// Catalog entry for a `.glb` decoration or prop asset served over HTTP(S).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub category: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkmask_roundtrip_and_bits() {
        let mut mask = WalkMask::all_walkable();
        assert!(mask.get(0, 0));
        assert!(mask.get(63, 63));
        mask.set(17, 42, false);
        assert!(!mask.get(17, 42));
        assert!(mask.get(16, 42));
        assert!(!mask.get(64, 0));

        let encoded = postcard::to_allocvec(&mask).unwrap();
        let decoded: WalkMask = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(mask, decoded);
    }

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
}

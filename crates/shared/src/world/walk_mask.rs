use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
}

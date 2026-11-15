use std::cmp::Ordering;

pub const TERRAIN_MATERIAL_COUNT: usize = 9;

pub const MAT_GRASS: u8 = 0;
pub const MAT_DIRT: u8 = 1;
pub const MAT_ROCK: u8 = 2;
pub const MAT_SAND: u8 = 3;
pub const MAT_LIGHT_GRASS: u8 = 4;
pub const MAT_DARK_GRASS: u8 = 5;
pub const MAT_PATCHY_GRASS: u8 = 6;
pub const MAT_COBBLESTONE: u8 = 7;
pub const MAT_BRICK: u8 = 8;

/// Packs 4 legacy RGBA8 texture-splat weights into a little-endian `u32`.
#[inline]
pub const fn pack_splat(layer0: u8, layer1: u8, layer2: u8, layer3: u8) -> u32 {
    u32::from_le_bytes([layer0, layer1, layer2, layer3])
}

/// Unpacks 4 legacy RGBA8 texture-splat weights from a little-endian `u32`.
#[inline]
pub const fn unpack_splat(packed: u32) -> [u8; 4] {
    packed.to_le_bytes()
}

#[inline]
pub const fn default_splat_ids() -> [u8; 4] {
    [MAT_GRASS, 0, 0, 0]
}

#[inline]
pub const fn default_splat_weights() -> [u8; 4] {
    [255, 0, 0, 0]
}

#[inline]
fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derives a deterministic 64-bit world-noise seed from an area's string identifier.
pub fn area_seed_from_id(area_id: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for &b in area_id.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    splitmix64(hash)
}

#[inline]
fn hash_grid_2d(seed: u64, ix: i32, iz: i32) -> f32 {
    let x = (ix as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let z = (iz as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let h = splitmix64(seed ^ x ^ z.rotate_left(29));
    let unit = ((h >> 40) as f32) / ((1u32 << 24) - 1) as f32;
    unit * 2.0 - 1.0
}

fn value_noise_2d(seed: u64, x: f32, z: f32) -> f32 {
    let fx = x.floor();
    let fz = z.floor();
    let ix = fx as i32;
    let iz = fz as i32;
    let tx = x - fx;
    let tz = z - fz;
    let ux = tx * tx * tx * (tx * (tx * 6.0 - 15.0) + 10.0);
    let uz = tz * tz * tz * (tz * (tz * 6.0 - 15.0) + 10.0);

    let v00 = hash_grid_2d(seed, ix, iz);
    let v10 = hash_grid_2d(seed, ix + 1, iz);
    let v01 = hash_grid_2d(seed, ix, iz + 1);
    let v11 = hash_grid_2d(seed, ix + 1, iz + 1);

    let nx0 = v00 + (v10 - v00) * ux;
    let nx1 = v01 + (v11 - v01) * ux;
    nx0 + (nx1 - nx0) * uz
}

fn fbm_grass_2d(seed: u64, wx: f32, wz: f32) -> f32 {
    0.58 * value_noise_2d(seed, wx / 22.0, wz / 22.0)
        + 0.28 * value_noise_2d(seed ^ 0x517C_C1B7_2722_0A95, wx / 9.5, wz / 9.5)
        + 0.14 * value_noise_2d(seed ^ 0x94D0_49BB_1331_11EB, wx / 4.5, wz / 4.5)
}

/// Evaluates continuous world-space 3-tone grass noise (`MAT_GRASS`, `MAT_LIGHT_GRASS`, `MAT_DARK_GRASS`)
/// at integer world vertex coordinates `(wx, wz)`, ensuring seamless cross-tile borders.
pub fn grass_noise_splat_at(seed: u64, wx: i32, wz: i32) -> ([u8; 4], [u8; 4]) {
    let x = wx as f32;
    let z = wz as f32;
    let t = (fbm_grass_2d(seed, x, z) * 2.2).clamp(-1.0, 1.0);
    let detail = 0.65 * value_noise_2d(seed ^ 0x9E37_79B9_7F4A_7C15, x / 15.0, z / 15.0)
        + 0.35 * value_noise_2d(seed ^ 0xBF58_476D_1CE4_E5B9, x / 6.5, z / 6.5);
    let d = (detail * 2.0).clamp(-1.0, 1.0);

    let light_raw = (t - 0.08).max(0.0) + 0.20 * d.max(0.0);
    let dark_raw = (-t - 0.08).max(0.0) + 0.20 * (-d).max(0.0);
    let w_light = light_raw * light_raw.sqrt() * 1.8;
    let w_dark = dark_raw * dark_raw.sqrt() * 1.8;
    let w_mid = (1.0 - t.abs() * 0.88).max(0.06) + 0.15 * (1.0 - d.abs());
    normalize_splat_entries(&[
        (MAT_GRASS, w_mid),
        (MAT_LIGHT_GRASS, w_light),
        (MAT_DARK_GRASS, w_dark),
    ])
}

/// Deduplicates, sorts descending by weight, keeps top-4 materials, and quantizes weights to sum to `255`.
pub fn normalize_splat_entries(entries: &[(u8, f32)]) -> ([u8; 4], [u8; 4]) {
    let mut combined: Vec<(u8, f32)> = Vec::with_capacity(entries.len());
    for &(mat_id, weight) in entries {
        if !weight.is_finite() || weight <= 1e-5 {
            continue;
        }
        if let Some(existing) = combined.iter_mut().find(|(id, _)| *id == mat_id) {
            existing.1 += weight;
        } else {
            combined.push((mat_id, weight));
        }
    }

    if combined.is_empty() {
        return (default_splat_ids(), default_splat_weights());
    }

    combined.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    combined.truncate(4);

    let total: f32 = combined.iter().map(|(_, w)| *w).sum();
    if !total.is_finite() || total <= 1e-5 {
        return (default_splat_ids(), default_splat_weights());
    }

    // Hamilton / largest-remainder allocation preserves descending order and guarantees sum == 255.
    let mut floored = [0u8; 4];
    let mut fracs = [0.0f32; 4];
    let mut sum_floor: u16 = 0;
    for (i, &(_, w)) in combined.iter().enumerate() {
        let exact = (w / total) * 255.0;
        let f = exact.floor().clamp(0.0, 255.0) as u8;
        floored[i] = f;
        fracs[i] = exact - f as f32;
        sum_floor += f as u16;
    }

    let rem = 255u16.saturating_sub(sum_floor) as usize;
    let mut order: Vec<usize> = (0..combined.len()).collect();
    order.sort_by(|&a, &b| {
        fracs[b]
            .partial_cmp(&fracs[a])
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });
    for &idx in order.iter().take(rem) {
        floored[idx] = floored[idx].saturating_add(1);
    }

    let mut out_ids = [0u8; 4];
    let mut out_weights = [0u8; 4];
    let mut write_idx = 0usize;
    for i in 0..combined.len() {
        if floored[i] > 0 {
            out_ids[write_idx] = combined[i].0;
            out_weights[write_idx] = floored[i];
            write_idx += 1;
        }
    }

    if write_idx == 0 {
        return (default_splat_ids(), default_splat_weights());
    }
    (out_ids, out_weights)
}

/// Blends `target_mat` into a vertex's top-4 material sample with strength `alpha` in `[0, 1]`.
pub fn blend_splat_sample(
    ids: [u8; 4],
    weights: [u8; 4],
    target_mat: u8,
    alpha: f32,
) -> ([u8; 4], [u8; 4]) {
    let a = alpha.clamp(0.0, 1.0);
    let keep = 1.0 - a;
    let mut active: Vec<(u8, f32)> = Vec::with_capacity(5);
    for i in 0..4 {
        if weights[i] > 0 {
            let scaled = (weights[i] as f32 / 255.0) * keep;
            if let Some(existing) = active.iter_mut().find(|(id, _)| *id == ids[i]) {
                existing.1 += scaled;
            } else {
                active.push((ids[i], scaled));
            }
        }
    }

    if a <= 1e-6 {
        return normalize_splat_entries(&active);
    }

    if let Some(existing) = active.iter_mut().find(|(id, _)| *id == target_mat) {
        existing.1 += a;
    } else {
        if active.len() >= 4 {
            active.sort_by(|lhs, rhs| {
                rhs.1
                    .partial_cmp(&lhs.1)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| lhs.0.cmp(&rhs.0))
            });
            active.truncate(3);
        }
        active.push((target_mat, a));
    }

    normalize_splat_entries(&active)
}

/// Converts a legacy `width × length` RGBA8 cell splat grid into a `(width + 1) × (length + 1)`
/// vertex-aligned `(splat_ids, splat_weights)` grid by averaging adjacent cells.
pub fn convert_legacy_rgba_splat_grid(
    legacy: &[u32],
    width: usize,
    length: usize,
) -> (Vec<[u8; 4]>, Vec<[u8; 4]>) {
    if legacy.is_empty() || width == 0 || length == 0 {
        return (Vec::new(), Vec::new());
    }

    let vw = width + 1;
    let vh = length + 1;
    let verts = vw * vh;
    if legacy.len() < width * length {
        return (
            vec![default_splat_ids(); verts],
            vec![default_splat_weights(); verts],
        );
    }

    let mut out_ids = Vec::with_capacity(verts);
    let mut out_weights = Vec::with_capacity(verts);
    for vz in 0..vh {
        let cz0 = vz.saturating_sub(1);
        let cz1 = vz.min(length - 1);
        for vx in 0..vw {
            let cx0 = vx.saturating_sub(1);
            let cx1 = vx.min(width - 1);
            let mut sums = [0.0f32; 4];
            for cz in cz0..=cz1 {
                for cx in cx0..=cx1 {
                    let c = unpack_splat(legacy[cz * width + cx]);
                    for ch in 0..4 {
                        sums[ch] += c[ch] as f32;
                    }
                }
            }
            let entries = [
                (MAT_GRASS, sums[0]),
                (MAT_DIRT, sums[1]),
                (MAT_ROCK, sums[2]),
                (MAT_SAND, sums[3]),
            ];
            let (ids, weights) = normalize_splat_entries(&entries);
            out_ids.push(ids);
            out_weights.push(weights);
        }
    }

    (out_ids, out_weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_supports_more_than_four_materials_and_keeps_sum_255() {
        let mut ids = default_splat_ids();
        let mut weights = default_splat_weights();

        for mat in [
            MAT_DIRT,
            MAT_ROCK,
            MAT_LIGHT_GRASS,
            MAT_COBBLESTONE,
            MAT_BRICK,
            MAT_PATCHY_GRASS,
        ] {
            (ids, weights) = blend_splat_sample(ids, weights, mat, 0.35);
            let sum: u16 = weights.iter().map(|&w| w as u16).sum();
            assert_eq!(sum, 255, "weights must always sum to 255 after blending mat {mat}");
            assert!(
                weights.windows(2).all(|w| w[0] >= w[1]),
                "weights must stay sorted descending: {weights:?}"
            );
            assert!(
                ids.iter().zip(weights.iter()).any(|(&id, &w)| id == mat && w > 0),
                "newly blended mat {mat} must appear in top 4: ids={ids:?}, weights={weights:?}"
            );
        }

        // Repeated painting with Brick saturates Brick to 255 and cleans unused slots.
        for _ in 0..30 {
            (ids, weights) = blend_splat_sample(ids, weights, MAT_BRICK, 0.5);
        }
        assert_eq!(ids, [MAT_BRICK, 0, 0, 0]);
        assert_eq!(weights, [255, 0, 0, 0]);
    }

    #[test]
    fn legacy_rgba_conversion_maps_channels_to_vertex_grid() {
        let mut legacy = vec![pack_splat(255, 0, 0, 0); 4];
        legacy[0] = pack_splat(0, 255, 0, 0);
        legacy[3] = pack_splat(0, 0, 128, 127);

        let (ids, weights) = convert_legacy_rgba_splat_grid(&legacy, 2, 2);
        assert_eq!(ids.len(), 9);
        assert_eq!(weights.len(), 9);
        // Top-left corner (0,0) touches only cell (0,0) => pure Dirt (mat 1)
        assert_eq!(ids[0], [MAT_DIRT, 0, 0, 0]);
        assert_eq!(weights[0], [255, 0, 0, 0]);
        // Center vertex (1,1) averages all 4 cells and keeps descending order summing to 255
        let center_sum: u16 = weights[4].iter().map(|&w| w as u16).sum();
        assert_eq!(center_sum, 255);
        assert_eq!(ids[4][0], MAT_GRASS);
    }
}

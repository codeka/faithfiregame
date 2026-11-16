use std::f32::consts::TAU;

use glow::HasContext;
use shared::world::TERRAIN_MATERIAL_COUNT;

pub(super) fn hash2(x: i32, y: i32) -> f32 {
    let n = (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(1274126177);
    ((n ^ (n >> 13)) as u32 & 0xFFFF) as f32 / 65535.0
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

pub(super) fn tileable_noise_aniso(u: f32, v: f32, period_u: i32, period_v: i32) -> f32 {
    let x = u * period_u as f32;
    let y = v * period_v as f32;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);

    let wrap_u = |val: i32| val.rem_euclid(period_u);
    let wrap_v = |val: i32| val.rem_euclid(period_v);
    let n00 = hash2(wrap_u(x0), wrap_v(y0));
    let n10 = hash2(wrap_u(x0 + 1), wrap_v(y0));
    let n01 = hash2(wrap_u(x0), wrap_v(y0 + 1));
    let n11 = hash2(wrap_u(x0 + 1), wrap_v(y0 + 1));

    let nx0 = n00 + (n10 - n00) * sx;
    let nx1 = n01 + (n11 - n01) * sx;
    nx0 + (nx1 - nx0) * sy
}

pub(super) fn tileable_noise(u: f32, v: f32, period: i32) -> f32 {
    tileable_noise_aniso(u, v, period, period)
}

pub(super) fn fbm_tileable(u: f32, v: f32) -> f32 {
    0.55 * tileable_noise(u, v, 8)
        + 0.30 * tileable_noise(u, v, 16)
        + 0.15 * tileable_noise(u, v, 32)
}

pub(super) fn wrap_voronoi(u: f32, v: f32, cells: i32) -> (f32, f32, i32, i32) {
    let px = u * cells as f32;
    let py = v * cells as f32;
    let gx = px.floor() as i32;
    let gy = py.floor() as i32;

    let mut d1 = f32::MAX;
    let mut d2 = f32::MAX;
    let mut id1 = (0, 0);

    for oy in -1..=1 {
        for ox in -1..=1 {
            let cx = gx + ox;
            let cy = gy + oy;
            let wx = cx.rem_euclid(cells);
            let wy = cy.rem_euclid(cells);
            let jx = cx as f32 + 0.18 + 0.64 * hash2(wx, wy);
            let jy = cy as f32 + 0.18 + 0.64 * hash2(wx + 71, wy + 137);
            let dist = ((px - jx).powi(2) + (py - jy).powi(2)).sqrt();
            if dist < d1 {
                d2 = d1;
                d1 = dist;
                id1 = (wx, wy);
            } else if dist < d2 {
                d2 = dist;
            }
        }
    }

    (d1, d2, id1.0, id1.1)
}

/// Seamless criss-crossing grass blade relief field in `[0, 1]`.
fn sample_grass_blades(u: f32, v: f32, phase_u: f32, phase_v: f32) -> f32 {
    let uu = (u + phase_u).rem_euclid(1.0);
    let vv = (v + phase_v).rem_euclid(1.0);
    let wx = (tileable_noise(uu, vv, 8) - 0.5) * 0.08;
    let wy = (tileable_noise((uu + 0.37).rem_euclid(1.0), (vv + 0.61).rem_euclid(1.0), 8) - 0.5) * 0.08;

    let a1 = (2.0 * uu + vv + wx).rem_euclid(1.0);
    let b1 = (-uu + 2.0 * vv + wy).rem_euclid(1.0);
    let strand1 = tileable_noise_aniso(a1, b1, 36, 10);

    let a2 = (uu - 2.0 * vv + wy + 0.29).rem_euclid(1.0);
    let b2 = (2.0 * uu + vv + wx + 0.53).rem_euclid(1.0);
    let strand2 = tileable_noise_aniso(a2, b2, 36, 10);

    let blade = smoothstep(0.34, 0.86, strand1.max(strand2));
    let tufts = tileable_noise(uu, vv, 20);
    (0.26 * tufts + 0.74 * blade).clamp(0.0, 1.0)
}

fn sample_material_rgba(layer: usize, u: f32, v: f32) -> [f32; 4] {
    let n = fbm_tileable(u, v);
    let fine = tileable_noise(u, v, 64);

    match layer {
        // 0: Meadow Grass
        0 => {
            let blades = sample_grass_blades(u, v, 0.0, 0.0);
            let lift = (n - 0.5) * 0.11 + (blades - 0.5) * 0.08 + (fine - 0.5) * 0.04;
            let h = (0.18 + 0.74 * blades + (n - 0.5) * 0.08).clamp(0.0, 1.0);
            [0.24 + lift * 0.55, 0.52 + lift, 0.18 + lift * 0.45, h]
        }
        // 1: Earthy Dirt
        1 => {
            let clods = tileable_noise(u, v, 24);
            let pebbles = smoothstep(
                0.72,
                0.92,
                tileable_noise((u + 0.41).rem_euclid(1.0), (v + 0.17).rem_euclid(1.0), 48),
            );
            let base = 0.48 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.06 + pebbles * 0.04;
            let h = (0.32 + 0.20 * clods + 0.10 * n + 0.12 * pebbles + (fine - 0.5) * 0.06)
                .clamp(0.0, 1.0);
            [base * 1.15, base * 0.85, base * 0.58, h]
        }
        // 2: Slate Rock
        2 => {
            let strata = tileable_noise(u, (v * 4.0).rem_euclid(1.0), 8);
            let (d1, d2, id_x, id_y) = wrap_voronoi(u, v, 7);
            let fissure = smoothstep(0.02, 0.11, d2 - d1);
            let plate_h = hash2(id_x + 17, id_y + 61);
            let s = (0.50
                + (n - 0.5) * 0.10
                + (strata - 0.5) * 0.08
                + (plate_h - 0.5) * 0.05
                + (fine - 0.5) * 0.05)
                * (0.78 + 0.22 * fissure);
            let crack_h = 0.14 + fine * 0.06;
            let shelf_h = 0.50
                + 0.22 * strata
                + 0.16 * plate_h
                + (n - 0.5) * 0.08
                + (fine - 0.5) * 0.04;
            let h = (crack_h + (shelf_h - crack_h) * fissure).clamp(0.0, 1.0);
            [s * 0.96, s * 0.98, s * 1.04, h]
        }
        // 3: Warm Sand
        3 => {
            let ripple_wave = (u * TAU * 4.0 + n * 2.5).sin() * 0.5 + 0.5;
            let s = 0.74 + (n - 0.5) * 0.10 + ripple_wave * 0.05 + (fine - 0.5) * 0.04;
            let h = (0.28 + 0.20 * ripple_wave + 0.10 * n + (fine - 0.5) * 0.05).clamp(0.0, 1.0);
            [s * 1.04, s * 0.95, s * 0.72, h]
        }
        // 4: Light Grass (sunlit spring meadow)
        4 => {
            let blades = sample_grass_blades(u, v, 0.37, 0.61);
            let lift = (n - 0.5) * 0.10 + (blades - 0.5) * 0.09 + (fine - 0.5) * 0.04;
            let h = (0.20 + 0.74 * blades + (n - 0.5) * 0.08).clamp(0.0, 1.0);
            [0.39 + lift * 0.8, 0.68 + lift, 0.24 + lift * 0.55, h]
        }
        // 5: Dark Grass (cool deep woodland turf)
        5 => {
            let mottle = tileable_noise(u, v, 12);
            let blades = sample_grass_blades(u, v, 0.71, 0.23);
            let shift = (n - 0.5) * 0.08
                + (mottle - 0.5) * 0.06
                + (blades - 0.5) * 0.07
                + (fine - 0.5) * 0.04;
            let h = (0.18 + 0.68 * blades + 0.10 * mottle + (n - 0.5) * 0.06).clamp(0.0, 1.0);
            [0.14 + shift * 0.55, 0.36 + shift, 0.16 + shift * 0.65, h]
        }
        // 6: Patchy Grass (tufted turf + exposed soil hybrid)
        6 => {
            let patch = 0.62 * tileable_noise(u, v, 6)
                + 0.38
                    * tileable_noise(
                        (u + 0.19).rem_euclid(1.0),
                        (v + 0.43).rem_euclid(1.0),
                        12,
                    );
            let grass_mask = smoothstep(0.42, 0.62, patch);
            let blades = sample_grass_blades(u, v, 0.19, 0.83);
            let dirt_rgb = {
                let base = 0.47 + (n - 0.5) * 0.11 + (fine - 0.5) * 0.07;
                [base * 1.13, base * 0.85, base * 0.59]
            };
            let turf_rgb = [
                0.23 + (n - 0.5) * 0.06,
                0.50 + (n - 0.5) * 0.12 + (blades - 0.5) * 0.07 + (fine - 0.5) * 0.05,
                0.17 + (n - 0.5) * 0.05,
            ];
            let rim = 4.0 * grass_mask * (1.0 - grass_mask);
            let blended = lerp_rgb(dirt_rgb, turf_rgb, grass_mask);
            let soil_h = 0.30 + 0.14 * n + (fine - 0.5) * 0.05;
            let turf_h = 0.38 + 0.54 * blades + (n - 0.5) * 0.06;
            let h = (soil_h + (turf_h - soil_h) * grass_mask).clamp(0.0, 1.0);
            [
                blended[0] + rim * 0.07,
                blended[1] + rim * 0.03,
                blended[2] - rim * 0.02,
                h,
            ]
        }
        // 7: Cobblestone (seamless Voronoi rounded stones + mortar)
        7 => {
            let (d1, d2, id_x, id_y) = wrap_voronoi(u, v, 10);
            let stone_mask = smoothstep(0.05, 0.13, d2 - d1);
            let crown = (1.0 - (d1 / 0.66).clamp(0.0, 1.0)).sqrt();
            let stone_seed = hash2(id_x + 31, id_y + 97);
            let warm_cool = hash2(id_x + 193, id_y + 53) - 0.5;
            let shade = 0.36 + 0.18 * crown + 0.14 * stone_seed + (fine - 0.5) * 0.07;
            let stone_rgb = [
                shade + warm_cool * 0.06,
                shade + warm_cool * 0.02,
                shade - warm_cool * 0.05,
            ];
            let mortar_rgb = [0.21 + (fine - 0.5) * 0.04, 0.20, 0.19];
            let rgb = lerp_rgb(mortar_rgb, stone_rgb, stone_mask);
            let mortar_h = 0.06 + fine * 0.06;
            let stone_h = 0.52 + 0.30 * crown + 0.12 * stone_seed + (fine - 0.5) * 0.04;
            let h = (mortar_h + (stone_h - mortar_h) * stone_mask).clamp(0.0, 1.0);
            [rgb[0], rgb[1], rgb[2], h]
        }
        // 8: Brick (running-bond clay/terracotta pavers)
        _ => {
            const COLS: i32 = 8;
            const ROWS: i32 = 16;
            let gy = v * ROWS as f32;
            let row = (gy.floor() as i32).rem_euclid(ROWS);
            let stagger = if row % 2 == 1 { 0.5 / COLS as f32 } else { 0.0 };
            let su = (u + stagger).rem_euclid(1.0);
            let gx = su * COLS as f32;
            let col = (gx.floor() as i32).rem_euclid(COLS);
            let fu = gx.fract();
            let fv = gy.fract();

            let edge_u = fu.min(1.0 - fu);
            let edge_v = fv.min(1.0 - fv);
            let brick_mask = smoothstep(0.045, 0.075, edge_u) * smoothstep(0.09, 0.15, edge_v);

            let kiln = hash2(col + 13, row + 47);
            let tint = hash2(col + 89, row + 131);
            let bevel = (1.0 - fv) * 0.05 + (fine - 0.5) * 0.05 + (n - 0.5) * 0.04;
            let brick_rgb = [
                0.49 + kiln * 0.14 + bevel,
                0.24 + tint * 0.07 + bevel * 0.65,
                0.19 + (1.0 - kiln) * 0.04 + bevel * 0.5,
            ];
            let mortar_rgb = [
                0.55 + (fine - 0.5) * 0.05,
                0.52 + (fine - 0.5) * 0.05,
                0.47 + (fine - 0.5) * 0.04,
            ];
            let rgb = lerp_rgb(mortar_rgb, brick_rgb, brick_mask);
            let mortar_h = 0.08 + fine * 0.06;
            let edge_bevel = smoothstep(0.05, 0.16, edge_u) * smoothstep(0.10, 0.24, edge_v);
            let brick_lift = hash2(col + 53, row + 19);
            let brick_h = 0.58 + 0.22 * edge_bevel + 0.12 * brick_lift + (fine - 0.5) * 0.04;
            let h = (mortar_h + (brick_h - mortar_h) * brick_mask).clamp(0.0, 1.0);
            [rgb[0], rgb[1], rgb[2], h]
        }
    }
}

pub(super) fn create_terrain_texture_array(gl: &glow::Context) -> Result<glow::Texture, String> {
    const SIZE: usize = 256;
    const LAYERS: usize = TERRAIN_MATERIAL_COUNT;
    let mut pixels = vec![0u8; SIZE * SIZE * LAYERS * 4];

    for layer in 0..LAYERS {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = x as f32 / SIZE as f32;
                let v = y as f32 / SIZE as f32;
                let rgba = sample_material_rgba(layer, u, v);

                let idx = ((layer * SIZE * SIZE) + y * SIZE + x) * 4;
                pixels[idx] = (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 1] = (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 2] = (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[idx + 3] = (rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        // NOTE: glow 0.16.0's web_sys `tex_image_3d` swaps `border` and `depth`.
        // Use immutable `tex_storage_3d` + `tex_sub_image_3d`, whose parameters are ordered properly.
        let mip_levels = (SIZE.ilog2() + 1) as i32;
        gl.tex_storage_3d(
            glow::TEXTURE_2D_ARRAY,
            mip_levels,
            glow::RGBA8,
            SIZE as i32,
            SIZE as i32,
            LAYERS as i32,
        );
        gl.tex_sub_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            0,
            0,
            0,
            SIZE as i32,
            SIZE as i32,
            LAYERS as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&pixels)),
        );
        gl.generate_mipmap(glow::TEXTURE_2D_ARRAY);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        Ok(tex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::world::{MAT_BRICK, MAT_COBBLESTONE, MAT_DARK_GRASS, MAT_DIRT, MAT_GRASS, MAT_LIGHT_GRASS, MAT_SAND};

    #[test]
    fn all_terrain_materials_are_valid_and_tile_seamlessly() {
        for layer in 0..TERRAIN_MATERIAL_COUNT {
            for i in 0..32 {
                let t = i as f32 / 32.0;
                let a = sample_material_rgba(layer, 0.0, t);
                let b = sample_material_rgba(layer, 0.999999, t);
                let c = sample_material_rgba(layer, t, 0.0);
                let d = sample_material_rgba(layer, t, 0.999999);
                for ch in 0..4 {
                    assert!(a[ch].is_finite() && (0.0..=1.0).contains(&a[ch]));
                    assert!(
                        (a[ch] - b[ch]).abs() < 0.02,
                        "U seam on layer {layer} channel {ch}: {a:?} vs {b:?}"
                    );
                    assert!(
                        (c[ch] - d[ch]).abs() < 0.02,
                        "V seam on layer {layer} channel {ch}: {c:?} vs {d:?}"
                    );
                }
            }
        }
    }

    fn height_blend_shares(w_a: f32, h_a: f32, w_b: f32, h_b: f32) -> (f32, f32) {
        const HEIGHT_STRENGTH: f32 = 0.85;
        const BLEND_DEPTH: f32 = 0.15;
        let s_a = w_a + HEIGHT_STRENGTH * h_a * smoothstep(0.0, 0.15, w_a);
        let s_b = w_b + HEIGHT_STRENGTH * h_b * smoothstep(0.0, 0.15, w_b);
        let threshold = s_a.max(s_b) - BLEND_DEPTH;
        let bw_a = smoothstep(0.0, 0.05, w_a) * (s_a - threshold).max(0.0);
        let bw_b = smoothstep(0.0, 0.05, w_b) * (s_b - threshold).max(0.0);
        let total = (bw_a + bw_b).max(1e-6);
        (bw_a / total, bw_b / total)
    }

    #[test]
    fn material_heightmaps_have_expected_relief_for_cracks_and_blades() {
        let mut min_h = [f32::MAX; TERRAIN_MATERIAL_COUNT];
        let mut max_h = [f32::MIN; TERRAIN_MATERIAL_COUNT];
        let mut min_pt = [(0.0f32, 0.0f32); TERRAIN_MATERIAL_COUNT];
        let mut max_pt = [(0.0f32, 0.0f32); TERRAIN_MATERIAL_COUNT];

        for layer in 0..TERRAIN_MATERIAL_COUNT {
            for y in 0..64 {
                for x in 0..64 {
                    let u = x as f32 / 64.0;
                    let v = y as f32 / 64.0;
                    let h = sample_material_rgba(layer, u, v)[3];
                    if h < min_h[layer] {
                        min_h[layer] = h;
                        min_pt[layer] = (u, v);
                    }
                    if h > max_h[layer] {
                        max_h[layer] = h;
                        max_pt[layer] = (u, v);
                    }
                }
            }
        }

        for layer in [MAT_COBBLESTONE as usize, MAT_BRICK as usize] {
            assert!(min_h[layer] < 0.16, "layer {layer} mortar too shallow: {}", min_h[layer]);
            assert!(max_h[layer] > 0.82, "layer {layer} crown/face too low: {}", max_h[layer]);
        }
        for layer in [MAT_GRASS as usize, MAT_LIGHT_GRASS as usize, MAT_DARK_GRASS as usize] {
            assert!(min_h[layer] < 0.30, "layer {layer} gaps too high: {}", min_h[layer]);
            assert!(max_h[layer] > 0.84, "layer {layer} blades too low: {}", max_h[layer]);
        }
        for layer in [MAT_DIRT as usize, MAT_SAND as usize] {
            assert!(min_h[layer] > 0.22, "layer {layer} floor too low: {}", min_h[layer]);
            assert!(max_h[layer] < 0.72, "layer {layer} peaks too high: {}", max_h[layer]);
        }

        // Cobblestone + Dirt (w_cobble = 0.65, w_dirt = 0.35):
        // mortar cracks receive Dirt (> 50%), while stone crowns stay 100% Cobblestone.
        let cob_crack = min_pt[MAT_COBBLESTONE as usize];
        let cob_crown = max_pt[MAT_COBBLESTONE as usize];
        let h_cob_crack = sample_material_rgba(MAT_COBBLESTONE as usize, cob_crack.0, cob_crack.1)[3];
        let h_dirt_crack = sample_material_rgba(MAT_DIRT as usize, cob_crack.0, cob_crack.1)[3];
        let (_, dirt_in_crack) = height_blend_shares(0.65, h_cob_crack, 0.35, h_dirt_crack);
        assert!(dirt_in_crack > 0.50, "dirt share in cobble crack = {dirt_in_crack}");

        let h_cob_crown = sample_material_rgba(MAT_COBBLESTONE as usize, cob_crown.0, cob_crown.1)[3];
        let h_dirt_crown = sample_material_rgba(MAT_DIRT as usize, cob_crown.0, cob_crown.1)[3];
        let (cob_on_crown, _) = height_blend_shares(0.65, h_cob_crown, 0.35, h_dirt_crown);
        assert!((cob_on_crown - 1.0).abs() < 1e-5, "cobble share on crown = {cob_on_crown}");

        // Grass + Sand (w_grass = 0.35, w_sand = 0.65):
        // tall grass blades poke through (> 50%), while inter-blade gaps resolve to 100% Sand.
        let grass_blade = max_pt[MAT_GRASS as usize];
        let grass_gap = min_pt[MAT_GRASS as usize];
        let h_grass_blade = sample_material_rgba(MAT_GRASS as usize, grass_blade.0, grass_blade.1)[3];
        let h_sand_blade = sample_material_rgba(MAT_SAND as usize, grass_blade.0, grass_blade.1)[3];
        let (grass_at_blade, _) = height_blend_shares(0.35, h_grass_blade, 0.65, h_sand_blade);
        assert!(grass_at_blade > 0.50, "grass share at blade peak = {grass_at_blade}");

        let h_grass_gap = sample_material_rgba(MAT_GRASS as usize, grass_gap.0, grass_gap.1)[3];
        let h_sand_gap = sample_material_rgba(MAT_SAND as usize, grass_gap.0, grass_gap.1)[3];
        let (_, sand_at_gap) = height_blend_shares(0.35, h_grass_gap, 0.65, h_sand_gap);
        assert!((sand_at_gap - 1.0).abs() < 1e-5, "sand share at grass gap = {sand_at_gap}");
    }
}

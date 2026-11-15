use std::f32::consts::TAU;

use glow::HasContext;
use shared::world::TERRAIN_MATERIAL_COUNT;

fn hash2(x: i32, y: i32) -> f32 {
    let n = (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(1274126177);
    ((n ^ (n >> 13)) as u32 & 0xFFFF) as f32 / 65535.0
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn tileable_noise(u: f32, v: f32, period: i32) -> f32 {
    let x = u * period as f32;
    let y = v * period as f32;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);

    let wrap = |v: i32| v.rem_euclid(period);
    let n00 = hash2(wrap(x0), wrap(y0));
    let n10 = hash2(wrap(x0 + 1), wrap(y0));
    let n01 = hash2(wrap(x0), wrap(y0 + 1));
    let n11 = hash2(wrap(x0 + 1), wrap(y0 + 1));

    let nx0 = n00 + (n10 - n00) * sx;
    let nx1 = n01 + (n11 - n01) * sx;
    nx0 + (nx1 - nx0) * sy
}

fn fbm_tileable(u: f32, v: f32) -> f32 {
    0.55 * tileable_noise(u, v, 8)
        + 0.30 * tileable_noise(u, v, 16)
        + 0.15 * tileable_noise(u, v, 32)
}

fn wrap_voronoi(u: f32, v: f32, cells: i32) -> (f32, f32, i32, i32) {
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

fn sample_material_rgb(layer: usize, u: f32, v: f32) -> [f32; 3] {
    let n = fbm_tileable(u, v);
    let fine = tileable_noise(u, v, 64);

    match layer {
        // 0: Meadow Grass
        0 => {
            let g = 0.52 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.05;
            [0.24 + (n - 0.5) * 0.06, g, 0.18 + (n - 0.5) * 0.05]
        }
        // 1: Earthy Dirt
        1 => {
            let base = 0.48 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.06;
            [base * 1.15, base * 0.85, base * 0.58]
        }
        // 2: Slate Rock
        2 => {
            let strata = tileable_noise(u, (v * 4.0).rem_euclid(1.0), 8);
            let s = 0.50 + (n - 0.5) * 0.10 + (strata - 0.5) * 0.08 + (fine - 0.5) * 0.05;
            [s * 0.96, s * 0.98, s * 1.04]
        }
        // 3: Warm Sand
        3 => {
            let ripple = ((u * TAU * 4.0 + n * 2.5).sin() * 0.5 + 0.5) * 0.05;
            let s = 0.74 + (n - 0.5) * 0.10 + ripple + (fine - 0.5) * 0.04;
            [s * 1.04, s * 0.95, s * 0.72]
        }
        // 4: Light Grass (sunlit spring meadow)
        4 => {
            let blades = tileable_noise((u * 2.0).rem_euclid(1.0), v, 48);
            let lift = (n - 0.5) * 0.10 + (blades - 0.5) * 0.07 + (fine - 0.5) * 0.04;
            [0.39 + lift * 0.8, 0.68 + lift, 0.24 + lift * 0.55]
        }
        // 5: Dark Grass (cool deep woodland turf)
        5 => {
            let mottle = tileable_noise(u, v, 12);
            let shift = (n - 0.5) * 0.08 + (mottle - 0.5) * 0.07 + (fine - 0.5) * 0.04;
            [0.14 + shift * 0.55, 0.36 + shift, 0.16 + shift * 0.65]
        }
        // 6: Patchy Grass (tufted turf + exposed soil hybrid)
        6 => {
            let patch = 0.62 * tileable_noise(u, v, 6)
                + 0.38 * tileable_noise((u + 0.19).rem_euclid(1.0), (v + 0.43).rem_euclid(1.0), 12);
            let grass_mask = smoothstep(0.42, 0.62, patch);
            let dirt_rgb = {
                let base = 0.47 + (n - 0.5) * 0.11 + (fine - 0.5) * 0.07;
                [base * 1.13, base * 0.85, base * 0.59]
            };
            let turf_rgb = [
                0.23 + (n - 0.5) * 0.06,
                0.50 + (n - 0.5) * 0.12 + (fine - 0.5) * 0.05,
                0.17 + (n - 0.5) * 0.05,
            ];
            let rim = 4.0 * grass_mask * (1.0 - grass_mask);
            let blended = lerp_rgb(dirt_rgb, turf_rgb, grass_mask);
            [
                blended[0] + rim * 0.07,
                blended[1] + rim * 0.03,
                blended[2] - rim * 0.02,
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
            lerp_rgb(mortar_rgb, stone_rgb, stone_mask)
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
            lerp_rgb(mortar_rgb, brick_rgb, brick_mask)
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
                let rgb = sample_material_rgb(layer, u, v);

                let idx = ((layer * SIZE * SIZE) + y * SIZE + x) * 4;
                pixels[idx] = (rgb[0].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 1] = (rgb[1].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 2] = (rgb[2].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 3] = 255;
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

    #[test]
    fn all_terrain_materials_are_valid_and_tile_seamlessly() {
        for layer in 0..TERRAIN_MATERIAL_COUNT {
            for i in 0..32 {
                let t = i as f32 / 32.0;
                let a = sample_material_rgb(layer, 0.0, t);
                let b = sample_material_rgb(layer, 0.999999, t);
                let c = sample_material_rgb(layer, t, 0.0);
                let d = sample_material_rgb(layer, t, 0.999999);
                for ch in 0..3 {
                    assert!(a[ch].is_finite() && (0.0..=1.0).contains(&a[ch]));
                    assert!((a[ch] - b[ch]).abs() < 0.02, "U seam on layer {layer}: {a:?} vs {b:?}");
                    assert!((c[ch] - d[ch]).abs() < 0.02, "V seam on layer {layer}: {c:?} vs {d:?}");
                }
            }
        }
    }
}

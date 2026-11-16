use glow::HasContext;
use shared::world::ROAD_MATERIAL_COUNT;

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

    let wrap = |val: i32| val.rem_euclid(period);
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

fn stone_curb_rgb(u: f32, v: f32, curb_w: f32, blocks_per_v: f32, fine: f32, warm: [f32; 3]) -> [f32; 3] {
    let side = if u < 0.5 { 0 } else { 1 };
    let local_u = if side == 0 {
        u / curb_w
    } else {
        (1.0 - u) / curb_w
    }
    .clamp(0.0, 1.0);
    let gv = v * blocks_per_v;
    let row = (gv.floor() as i32).rem_euclid(blocks_per_v as i32);
    let fv = gv.fract();

    let joint_v = smoothstep(0.04, 0.10, fv.min(1.0 - fv));
    let inner_bevel = smoothstep(0.0, 0.20, local_u);
    let outer_bevel = smoothstep(0.0, 0.18, 1.0 - local_u);
    let block_mask = joint_v * (0.72 + 0.28 * inner_bevel * outer_bevel);
    let tone = hash2(row + 19 + side * 73, 101 + side * 41);
    let shade = 0.86 + tone * 0.20 + (fine - 0.5) * 0.08;
    let mortar = [warm[0] * 0.50, warm[1] * 0.50, warm[2] * 0.50];
    let block = [warm[0] * shade, warm[1] * shade, warm[2] * shade];
    lerp_rgb(mortar, block, block_mask)
}

fn sample_road_rgba(layer: usize, u: f32, v: f32) -> [f32; 4] {
    let n = fbm_tileable(u, v);
    let fine = tileable_noise(u, v, 64);

    match layer {
        // 0: Dirt Path (packed soil + twin wheel ruts + organic feathered edges)
        0 => {
            let rut_l = (-((u - 0.30) / 0.085).powi(2)).exp();
            let rut_r = (-((u - 0.70) / 0.085).powi(2)).exp();
            let ruts = (rut_l + rut_r).clamp(0.0, 1.0);
            let center_wear = (-((u - 0.50) / 0.14).powi(2)).exp();
            let grit = tileable_noise(u, v, 32);

            let base = 0.49 + (n - 0.5) * 0.13 + (fine - 0.5) * 0.08 + (grit - 0.5) * 0.05;
            let soil = [base * 1.13, base * 0.85, base * 0.57];
            let rut_rgb = [soil[0] * 0.78, soil[1] * 0.76, soil[2] * 0.74];
            let dusty_rgb = [soil[0] * 1.08, soil[1] * 1.04, soil[2] * 0.96];
            let mut rgb = lerp_rgb(soil, dusty_rgb, center_wear * 0.35);
            rgb = lerp_rgb(rgb, rut_rgb, ruts * 0.42);

            let edge_d = u.min(1.0 - u);
            let jittered = edge_d + (n - 0.5) * 0.055 + (grit - 0.5) * 0.025;
            let hard_mask = smoothstep(0.012, 0.032, edge_d);
            let alpha = (smoothstep(0.025, 0.14, jittered) * hard_mask).clamp(0.0, 1.0);
            [rgb[0], rgb[1], rgb[2], alpha]
        }
        // 1: Cobblestone with dressed stone curbstones
        1 => {
            const CURB: f32 = 0.085;
            if u < CURB || u > (1.0 - CURB) {
                let c = stone_curb_rgb(u, v, CURB, 8.0, fine, [0.56, 0.55, 0.53]);
                return [c[0], c[1], c[2], 1.0];
            }
            let inner_u = ((u - CURB) / (1.0 - 2.0 * CURB)).clamp(0.0, 1.0);
            let (d1, d2, id_x, id_y) = wrap_voronoi(inner_u, v, 10);
            let stone_mask = smoothstep(0.05, 0.13, d2 - d1);
            let crown = (1.0 - (d1 / 0.65).clamp(0.0, 1.0)).sqrt();
            let seed = hash2(id_x + 41, id_y + 83);
            let tint = hash2(id_x + 173, id_y + 29) - 0.5;
            let shade = 0.38 + 0.18 * crown + 0.13 * seed + (fine - 0.5) * 0.07;
            let stone_rgb = [shade + tint * 0.06, shade + tint * 0.02, shade - tint * 0.05];
            let mortar_rgb = [0.22 + (fine - 0.5) * 0.04, 0.21, 0.20];
            let rgb = lerp_rgb(mortar_rgb, stone_rgb, stone_mask);
            [rgb[0], rgb[1], rgb[2], 1.0]
        }
        // 2: Brick with soldier-course brick borders
        2 => {
            const CURB: f32 = 0.085;
            if u < CURB || u > (1.0 - CURB) {
                let c = stone_curb_rgb(u, v, CURB, 14.0, fine, [0.55, 0.28, 0.21]);
                return [c[0], c[1], c[2], 1.0];
            }
            const COLS: i32 = 6;
            const ROWS: i32 = 16;
            let inner_u = ((u - CURB) / (1.0 - 2.0 * CURB)).clamp(0.0, 1.0);
            let gy = v * ROWS as f32;
            let row = (gy.floor() as i32).rem_euclid(ROWS);
            let stagger = if row % 2 == 1 { 0.5 / COLS as f32 } else { 0.0 };
            let su = (inner_u + stagger).fract();
            let gx = su * COLS as f32;
            let col = (gx.floor() as i32).rem_euclid(COLS);
            let fu = gx.fract();
            let fv = gy.fract();

            let brick_mask =
                smoothstep(0.045, 0.075, fu.min(1.0 - fu)) * smoothstep(0.09, 0.15, fv.min(1.0 - fv));
            let kiln = hash2(col + 17, row + 59);
            let tint = hash2(col + 97, row + 139);
            let bevel = (1.0 - fv) * 0.05 + (fine - 0.5) * 0.05 + (n - 0.5) * 0.04;
            let brick_rgb = [
                0.50 + kiln * 0.13 + bevel,
                0.24 + tint * 0.07 + bevel * 0.65,
                0.19 + (1.0 - kiln) * 0.04 + bevel * 0.5,
            ];
            let mortar_rgb = [0.54 + (fine - 0.5) * 0.04, 0.51, 0.46];
            let rgb = lerp_rgb(mortar_rgb, brick_rgb, brick_mask);
            [rgb[0], rgb[1], rgb[2], 1.0]
        }
        // 3: Asphalt tarmac with pale concrete curbs
        3 => {
            const CURB: f32 = 0.070;
            if u < CURB || u > (1.0 - CURB) {
                let c = stone_curb_rgb(u, v, CURB, 4.0, fine, [0.64, 0.64, 0.62]);
                return [c[0], c[1], c[2], 1.0];
            }
            let aggregate = tileable_noise(u, v, 48);
            let sparkle = tileable_noise(u, v, 96);
            let spec = smoothstep(0.80, 0.94, sparkle) * 0.045;
            let wear = ((-((u - 0.33) / 0.10).powi(2)).exp() + (-((u - 0.67) / 0.10).powi(2)).exp())
                .clamp(0.0, 1.0);
            let base = 0.19 + (n - 0.5) * 0.04 + (aggregate - 0.5) * 0.04 + (fine - 0.5) * 0.03 + spec - wear * 0.025;
            [base * 0.98, base, base * 1.04, 1.0]
        }
        // 4: Flagstone slabs with dressed border stones
        _ => {
            const CURB: f32 = 0.080;
            if u < CURB || u > (1.0 - CURB) {
                let c = stone_curb_rgb(u, v, CURB, 6.0, fine, [0.52, 0.54, 0.51]);
                return [c[0], c[1], c[2], 1.0];
            }
            let inner_u = ((u - CURB) / (1.0 - 2.0 * CURB)).clamp(0.0, 1.0);
            let (d1, d2, id_x, id_y) = wrap_voronoi(inner_u, v, 5);
            let seam = smoothstep(0.035, 0.085, d2 - d1);
            let slab_tone = hash2(id_x + 23, id_y + 71);
            let warm_cool = hash2(id_x + 157, id_y + 11) - 0.5;
            let flat_tex = 0.45 + 0.14 * slab_tone + (n - 0.5) * 0.07 + (fine - 0.5) * 0.05;
            let slab_rgb = [
                flat_tex + warm_cool * 0.07,
                flat_tex + 0.02 + warm_cool * 0.02,
                flat_tex - warm_cool * 0.05,
            ];
            let mortar_rgb = [0.27 + (fine - 0.5) * 0.04, 0.27, 0.25];
            let rgb = lerp_rgb(mortar_rgb, slab_rgb, seam);
            [rgb[0], rgb[1], rgb[2], 1.0]
        }
    }
}

pub(super) fn create_road_texture_array(gl: &glow::Context) -> Result<glow::Texture, String> {
    const SIZE: usize = 256;
    const LAYERS: usize = ROAD_MATERIAL_COUNT;
    let mut pixels = vec![0u8; SIZE * SIZE * LAYERS * 4];

    for layer in 0..LAYERS {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = x as f32 / (SIZE - 1) as f32;
                let v = y as f32 / SIZE as f32;
                let rgba = sample_road_rgba(layer, u, v);
                let idx = ((layer * SIZE * SIZE) + y * SIZE + x) * 4;
                pixels[idx] = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 1] = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 2] = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 3] = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }

    unsafe {
        let tex = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
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
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        Ok(tex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn road_textures_are_valid_tile_in_v_and_feather_dirt_edges() {
        for layer in 0..ROAD_MATERIAL_COUNT {
            for i in 0..32 {
                let u = i as f32 / 31.0;
                let a = sample_road_rgba(layer, u, 0.0);
                let b = sample_road_rgba(layer, u, 0.999999);
                for ch in 0..4 {
                    assert!(a[ch].is_finite() && (0.0..=1.0).contains(&a[ch]));
                    assert!(
                        (a[ch] - b[ch]).abs() < 0.02,
                        "V seam on road layer {layer} u={u}: {a:?} vs {b:?}"
                    );
                }
                if layer == 0 {
                    assert!(sample_road_rgba(0, 0.0, 0.5)[3] < 0.01);
                    assert!(sample_road_rgba(0, 1.0, 0.5)[3] < 0.01);
                    assert!(sample_road_rgba(0, 0.5, 0.5)[3] > 0.95);
                } else {
                    assert!((a[3] - 1.0).abs() < 1e-5);
                }
            }
        }
    }
}

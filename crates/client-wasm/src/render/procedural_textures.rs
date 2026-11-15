use std::f32::consts::TAU;

use glow::HasContext;

fn hash2(x: i32, y: i32) -> f32 {
    let n = (x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(1274126177);
    ((n ^ (n >> 13)) as u32 & 0xFFFF) as f32 / 65535.0
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

pub(super) fn create_terrain_texture_array(gl: &glow::Context) -> Result<glow::Texture, String> {
    const SIZE: usize = 256;
    const LAYERS: usize = 4;
    let mut pixels = vec![0u8; SIZE * SIZE * LAYERS * 4];

    for layer in 0..LAYERS {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = x as f32 / SIZE as f32;
                let v = y as f32 / SIZE as f32;
                let n = fbm_tileable(u, v);
                let fine = tileable_noise(u, v, 64);

                let rgb: [f32; 3] = match layer {
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
                        let strata = tileable_noise(u * 0.5, v * 2.0, 16);
                        let s = 0.50 + (n - 0.5) * 0.10 + (strata - 0.5) * 0.08 + (fine - 0.5) * 0.05;
                        [s * 0.96, s * 0.98, s * 1.04]
                    }
                    // 3: Warm Sand
                    _ => {
                        let ripple = ((u * TAU * 4.0 + n * 2.5).sin() * 0.5 + 0.5) * 0.05;
                        let s = 0.74 + (n - 0.5) * 0.10 + ripple + (fine - 0.5) * 0.04;
                        [s * 1.04, s * 0.95, s * 0.72]
                    }
                };

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
